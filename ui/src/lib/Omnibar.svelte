<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly, scale, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import { api, brain, type ComposerMode, type PlanQuote, type Project, type ProviderStatus } from "./api";
  import { remote as server } from "./remote";
  import { effortsFor, fitEffort, seesImages, shownOf } from "./providerRows";
  import { permissionBlocked } from "./lanes";
  import { folderName, toAddress } from "./tabs";
  import { relativeTo, sameDir, shortAttachment as shortName } from "./paths";
  import { popover, placeAbove } from "./popover";
  import { bookmarks, completeAddress, history, pins } from "./browserData";
  import SuggestList from "./SuggestList.svelte";
  import { tooltip } from "./tooltip";
  import { boxText, firstRows, looksLikeUrl, mergeRows, pageRows, phraseRows, type Suggestion } from "./suggest";
  import type { IconName } from "./icons";
  import { viewImage } from "./imageViewer";

  export let input = "";
  export let model = "";
  export let effort = "medium";
  // Never defaults to full access: App seeds it from Settings' default.
  export let permission = "supervised";
  // Settings' default_mode; options it would ignore are shown disabled.
  export let policy = "";
  // "search" talks to the web; anything else to the agent. Whether the
  // agent builds or assists follows the folder, so it is no choice here.
  export let mode: ComposerMode = "build";
  export let attachments: string[] = [];
  // Plan items quoted from the plan panel: a message can be just these.
  export let quotes: PlanQuote[] = [];
  export let folder = "";
  export let folderLocked = false;
  // A session has started here: everything goes to its agent, so Web
  // (opening a page from the composer) is for a new tab only.
  export let started = false;
  $: if (started && mode === "search") mode = "build";
  export let branch = "";
  export let streaming = false;
  // A message sent while the agent works: it goes out when the run is done.
  export let held = "";
  // A session is open: Ctrl+Enter can start a side task beside it.
  export let canSide = false;
  export let board: ProviderStatus[] = [];
  export let hero = false;
  export let project: { slug: string; title: string; tokens: number } | null = null;
  // The linked server ("" when none). `remote` sends this composer's
  // sessions there; a started session stays where it began (`remoteLocked`).
  export let remoteHost = "";
  export let remote = false;
  export let remoteLocked = false;
  // The server folder this composer's new session runs in ("" = scratch).
  export let remoteCwd = "";

  const dispatch = createEventDispatcher<{
    send: void;
    unhold: void;
    side: void;
    sideHeld: void;
    sendHeldNow: void;
    browse: { url: string };
    stop: void;
    command: { name: string };
    unavailable: { provider: string };
    folder: { path: string };
    project: void;
    brain: void;
    remote: { on: boolean };
    remoteFolder: { path: string };
  }>();

  // ids are the `mode` strings the backend reads (ApprovalMode::parse:
  // "auto" and "full" run, anything else asks). Three choices, done.
  const PERMS: { id: string; title: string; label: string; desc: string; icon: IconName }[] = [
    { id: "supervised", title: "Ask", label: "Ask", desc: "Ask before commands and file changes.", icon: "lock" },
    { id: "auto", title: "Auto", label: "Auto", desc: "Approve routine actions without asking.", icon: "spark" },
    { id: "full", title: "Full", label: "Full", desc: "Run commands and edits without asking.", icon: "unlock" },
  ];

  const SLASH = [
    { name: "agent", hint: "Ask the agent" },
    { name: "web", hint: "Search the web" },
    { name: "new", hint: "New session" },
    { name: "fork", hint: "Branch this session" },
    { name: "compact", hint: "Summarize to free context" },
    { name: "stop", hint: "Stop the run" },
    { name: "model", hint: "Pick a model" },
    { name: "effort", hint: "Cycle effort" },
    { name: "page", hint: "Open a web page" },
    { name: "settings", hint: "Open settings" },
  ];

  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  const MAX_ATTACH = 8;

  let textarea: HTMLTextAreaElement | null = null;
  let picker: ModelPicker;
  let permBtn: HTMLButtonElement | null = null;
  let permOpen = false;
  let permStyle = "";
  let projBtn: HTMLButtonElement | null = null;
  // Where a new session works: the folder as it is (""), or a worktree of
  // its own made from this branch.
  export let worktree = "";
  let branchBtn: HTMLButtonElement | null = null;
  let branchOpen = false;
  let branchStyle = "";
  let branchList: string[] = [];
  // Another folder, another repository: back to working in it as it is.
  let worktreeFor = folder;
  $: if (folder !== worktreeFor) {
    worktreeFor = folder;
    worktree = "";
  }

  async function toggleBranch() {
    if (folderLocked) return;
    branchOpen = !branchOpen;
    if (!branchOpen || !branchBtn) return;
    branchStyle = placeAbove(branchBtn, 280);
    branchList = (await api.gitBranches(folder).catch(() => ({ current: branch, all: [] as string[] }))).all;
  }

  function pickBranch(base: string) {
    worktree = base;
    branchOpen = false;
    textarea?.focus();
  }
  let projOpen = false;
  let projStyle = "";
  let projects: Project[] = [];
  let projErr = "";
  let atItems: string[] = [];
  let atIndex = 0;
  let slashIndex = 0;
  let thumbs: Record<string, string | null> = {};
  let attachError = "";
  let attachErrorTimer = 0;
  // A composer error says its piece and goes: it never outlives the moment.
  $: if (attachError) {
    clearTimeout(attachErrorTimer);
    attachErrorTimer = window.setTimeout(() => (attachError = ""), 6000);
  }
  let webRows: Suggestion[] = [];
  let webActive = -1;
  let webTyped = "";
  let webSeq = 0;
  let webTimer = 0;
  let atSeq = 0;
  let atTimer = 0;
  let thumbsKey = "";
  let focused = false;

  $: efforts = effortsFor(model, board);
  $: {
    const next = fitEffort(effort, efforts);
    if (next !== effort) effort = next;
  }
  // A stored choice from before the menu shrank (read-only, auto-edits)
  // falls back to Supervised rather than a blank button.
  $: perm = PERMS.find((p) => p.id === permission) ?? PERMS[0];
  $: slashQuery = /^\/\w*$/.test(input.trim()) ? input.trim().slice(1).toLowerCase() : null;
  $: slashItems =
    slashQuery === null ? [] : SLASH.filter((c) => c.name.startsWith(slashQuery) && !(started && (c.name === "web" || c.name === "agent")));
  $: if (slashIndex >= slashItems.length) slashIndex = 0;
  // Attachments change rarely; guard by key so per-keystroke parent
  // object churn (same array, new dirty flag) doesn't re-decode images.
  $: {
    const files = Array.isArray(attachments) ? attachments : [];
    const key = `${folder}\n${files.join("\n")}`;
    if (key !== thumbsKey) {
      thumbsKey = key;
      void loadThumbs(files, folder);
    }
  }
  // `export let quotes = []` does not refill after mount. A bound composer
  // with no quote list hands over undefined, and reading `.length` here
  // aborts the flush so later clicks never update the UI.
  $: if (!Array.isArray(quotes)) quotes = [];
  $: if (!Array.isArray(attachments)) attachments = [];

  export function focus() {
    textarea?.focus();
  }

  export function openModels() {
    picker?.show();
  }

  $: web = mode === "search";
  $: placeholder = streaming
    ? canSide && !web
      ? "Working · Enter waits for it, Ctrl+Enter runs beside it"
      : "Working · Esc to stop"
    : web
      ? "Search the web or type an address"
      : folder || remoteCwd
        ? "Describe a task or ask a question"
        : "Ask anything";
  // An image is attached and the chosen model can't see it: said before send.
  $: blind = !web && !!model && (attachments ?? []).some((a) => IMAGE.test(a)) && !seesImages(model, board);
  $: canSubmit = !!input.trim() || ((quotes?.length ?? 0) > 0 && !web);

  function setWeb(on: boolean) {
    mode = on ? "search" : "build";
    if (on) projOpen = false;
    textarea?.focus();
  }

  function submit() {
    const text = input.trim();
    if (!canSubmit) return;
    if (mode === "search") {
      if (webActive >= 0 && webRows[webActive]) {
        pickWeb(webRows[webActive]);
        return;
      }
      clearWeb();
      dispatch("browse", { url: toAddress(text) });
      input = "";
    } else {
      dispatch("send");
    }
  }

  function runSlash(name: string) {
    input = "";
    if (name === "model") picker?.show();
    else if (name === "effort") cycleEffort();
    else if (name === "web" || name === "agent") setWeb(name === "web");
    else dispatch("command", { name });
  }

  function cycleEffort() {
    if (!efforts.length) return;
    effort = efforts[(Math.max(0, efforts.indexOf(effort)) + 1) % efforts.length];
  }

  function move(list: unknown[], i: number, delta: number) {
    return (i + delta + list.length) % Math.max(1, list.length);
  }

  function onKeydown(e: KeyboardEvent) {
    if (webOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      webActive = webActive + step < -1 ? webRows.length - 1 : webActive + step >= webRows.length ? -1 : webActive + step;
      input = webActive >= 0 ? boxText(webRows[webActive]) : webTyped;
      return;
    }
    if (webOpen && e.key === "Escape") {
      e.stopPropagation();
      input = webTyped;
      clearWeb();
      return;
    }
    if (slashItems.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      slashIndex = move(slashItems, slashIndex, e.key === "ArrowDown" ? 1 : -1);
    } else if (atItems.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      atIndex = move(atItems, atIndex, e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && canSide && !web) {
      e.preventDefault();
      if (input.trim() || quotes.length) dispatch("side");
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (slashItems.length) runSlash(slashItems[slashIndex].name);
      else if (atItems.length) pickMention(atItems[atIndex]);
      else submit();
    } else if (!started && /^[12]$/.test(e.key) && (e.ctrlKey || e.metaKey) && !slashItems.length && !atItems.length) {
      e.preventDefault();
      setWeb(e.key === "2");
    } else if (e.key === "Escape" && (slashItems.length || atItems.length)) {
      e.stopPropagation();
      atItems = [];
      input = input.replace(/^\/\w*$/, "");
    }
  }

  // Fits the box to its text (up to 180px). Writes go through a local:
  // `textarea.style.x = …` marks `textarea` dirty in Svelte, which re-ran
  // the `$:` below after every await and froze the composer on the first
  // keystroke (an endless microtask loop).
  async function autosize(text: string) {
    await tick();
    const el = textarea;
    if (!el) return;
    if (!text) {
      el.style.height = "";
      return;
    }
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }

  // Keystrokes and programmatic fills (edit-load, tab drafts) both land here.
  $: if (textarea) void autosize(input);
  $: if (mode !== "search" || !input.trim()) clearWeb();
  $: webOpen = focused && mode === "search" && webRows.length > 0;

  function clearWeb() {
    webRows = [];
    webActive = -1;
    clearTimeout(webTimer);
  }

  function suggestWeb(value: string, completed: string) {
    const marks = [...$bookmarks, ...$pins];
    webRows = mergeRows(firstRows(value, completed), pageRows(value, $history, marks));
    clearTimeout(webTimer);
    if (!value.trim() || looksLikeUrl(value)) return;
    const seq = ++webSeq;
    webTimer = window.setTimeout(async () => {
      const phrases = await api.searchSuggest(value).catch(() => [] as string[]);
      if (seq !== webSeq || mode !== "search" || !input.trim()) return;
      webRows = mergeRows(firstRows(value, completed), phraseRows(value, phrases), pageRows(value, $history, marks));
    }, 120);
  }

  function pickWeb(row: Suggestion) {
    clearWeb();
    input = "";
    dispatch("browse", { url: row.url });
  }

  async function onInput(e: Event) {
    if (mode === "search" && textarea) {
      const typed = textarea.value;
      webTyped = typed;
      webActive = -1;
      let completed = "";
      if ((e as InputEvent).inputType?.startsWith("insert") && textarea.selectionStart === typed.length) {
        const full = completeAddress(typed, $history, [...$bookmarks, ...$pins]);
        if (full) {
          completed = typed + full.slice(typed.length);
          input = completed;
          await tick();
          textarea?.setSelectionRange(typed.length, completed.length);
        }
      }
      suggestWeb(typed, completed);
      return;
    }
    const at = !web ? /@([\w./-]*)$/.exec(input) : null;
    if (!at || !folder) {
      atItems = [];
      clearTimeout(atTimer);
      return;
    }
    // Debounce FS walks: one in-flight query per pause, stale wins dropped.
    const query = at[1].split("/").pop() ?? "";
    const seq = ++atSeq;
    clearTimeout(atTimer);
    atTimer = window.setTimeout(async () => {
      try {
        const found = await api.listFiles(folder, query);
        if (seq !== atSeq) return;
        atItems = found.slice(0, 8);
        atIndex = 0;
      } catch {
        if (seq === atSeq) atItems = [];
      }
    }, 150);
  }

  function pickMention(item: string) {
    input = input.replace(/@[\w./-]*$/, `@${item} `);
    addAttachments([item]);
    atItems = [];
    textarea?.focus();
  }

  function addAttachments(paths: string[]) {
    const next = paths.map((p) => relativeTo(p, folder)).filter((p) => p && !attachments.includes(p));
    if (attachments.length + next.length > MAX_ATTACH) attachError = `Up to ${MAX_ATTACH} files a message: the rest were left out.`;
    if (next.length) attachments = [...attachments, ...next].slice(0, MAX_ATTACH);
  }

  async function loadThumbs(list: string[], root: string) {
    const keep: Record<string, string | null> = {};
    for (const a of list) {
      if (!IMAGE.test(a)) continue;
      keep[a] = a in thumbs ? thumbs[a] : await api.readImageDataUrl(a, root).catch(() => null);
    }
    thumbs = keep;
  }

  async function pickFiles() {
    try {
      const picked = await open({ multiple: true, directory: false, defaultPath: folder || undefined });
      if (!picked) return;
      addAttachments(Array.isArray(picked) ? picked : [picked]);
    } catch (e) {
      attachError = String(e);
    }
  }

  async function toggleProject() {
    if (folderLocked) return;
    projOpen = !projOpen;
    if (!projOpen || !projBtn) return;
    projStyle = placeAbove(projBtn, 260);
    projErr = "";
    try {
      // With Remote on these are the server's projects: same notes, its folders.
      projects = remote ? await server.projects() : await brain.projects();
    } catch (e) {
      projects = [];
      projErr = String(e);
    }
  }

  // A project synced from elsewhere may have no folder on this machine
  // (or on the server) yet: choosing it asks for one first.
  async function chooseProject(p: Project | null) {
    projOpen = false;
    try {
      if (remote) {
        let path = p?.folder ?? "";
        if (p && !path) {
          const typed = window.prompt(`Folder for ${p.title} on ${remoteHost}`, "/home/");
          if (!typed?.trim()) return;
          path = (await server.projectFolder(p.slug, p.title, typed.trim())).folder;
        }
        dispatch("remoteFolder", { path });
        return;
      }
      let path = p?.folder ?? "";
      if (p && !path) {
        const picked = await api.pickFolder(folder);
        if (!picked) return;
        path = (await brain.upsertProject(p.title, picked, p.slug)).folder;
        dispatch("project");
      }
      dispatch("folder", { path });
    } catch (e) {
      attachError = String(e);
    }
  }

  async function newRemoteProject() {
    projOpen = false;
    const typed = window.prompt(`Folder on ${remoteHost} for the new project`, "/home/");
    if (!typed?.trim()) return;
    try {
      const p = await server.projectFolder(null, folderName(typed.trim()), typed.trim());
      dispatch("remoteFolder", { path: p.folder });
    } catch (e) {
      attachError = String(e);
    }
  }

  $: remoteProject = remote && remoteCwd ? folderName(remoteCwd) : "";

  async function adopt(dir: string) {
    const known = projects.find((p) => sameDir(p.folder, dir));
    if (!known) await brain.upsertProject(folderName(dir), dir);
    if (!sameDir(dir, folder)) dispatch("folder", { path: dir });
    dispatch("project");
  }

  async function newProject() {
    projOpen = false;
    try {
      const picked = await api.pickFolder(folder);
      if (picked) await adopt(picked);
    } catch (e) {
      attachError = String(e);
    }
  }

  async function makeProject() {
    projOpen = false;
    try {
      await adopt(folder);
    } catch (e) {
      attachError = String(e);
    }
  }

  function readDataUrl(file: File) {
    return new Promise<string>((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result ?? ""));
      r.onerror = () => reject(r.error);
      r.readAsDataURL(file);
    });
  }

  async function stageImages(files: FileList) {
    attachError = "";
    const images = [...files].filter((f) => f.type.startsWith("image/") || IMAGE.test(f.name));
    const room = Math.max(0, MAX_ATTACH - attachments.length);
    if (images.length > room) attachError = `Up to ${MAX_ATTACH} files a message: the rest were left out.`;
    for (const f of images.slice(0, room)) {
      try {
        const b64 = (await readDataUrl(f)).split(",", 2)[1] ?? "";
        if (b64) addAttachments([await api.stageImage(f.name || "pasted.png", b64)]);
      } catch (e) {
        attachError = `Couldn't attach ${f.name || "image"}: ${e}`;
      }
    }
  }

  function onPaste(e: ClipboardEvent) {
    const files = e.clipboardData?.files;
    if (!files?.length || ![...files].some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void stageImages(files);
  }

  function onDrop(e: DragEvent) {
    const files = e.dataTransfer?.files;
    if (files?.length) void stageImages(files);
  }

  function togglePerm() {
    permOpen = !permOpen;
    if (permOpen && permBtn) permStyle = placeAbove(permBtn, 280);
  }
</script>

<div class="ob" class:hero>
  <!-- The project rides on the box's top edge: where this session works.
       A web search has no project, so it folds away behind the box. -->
  {#if remote}
    <button
      bind:this={projBtn}
      class="proj set"
      class:folded={web}
      aria-hidden={web || undefined}
      tabindex={web ? -1 : undefined}
      aria-haspopup={folderLocked ? undefined : "menu"}
      aria-expanded={folderLocked ? undefined : projOpen}
      on:click|stopPropagation={toggleProject}
      use:tooltip={remoteCwd ? `Runs on ${remoteHost} in ${remoteCwd}` : `Runs on ${remoteHost}, in its own scratch folder`}
    >
      <Icon name="server" size={12} />
      <span class="proj-title">{remoteProject ? `${remoteProject} · ${remoteHost}` : remoteHost}</span>
      {#if !folderLocked}<Icon name="chevDown" size={12} />{/if}
    </button>
  {:else}
    <div class="tops">
      <button
        bind:this={projBtn}
        class="proj"
        class:set={!!project}
        class:folded={web}
        aria-hidden={web || undefined}
        tabindex={web ? -1 : undefined}
        aria-haspopup={folderLocked ? undefined : "menu"}
        aria-expanded={folderLocked ? undefined : projOpen}
        on:click|stopPropagation={toggleProject}
        use:tooltip={project ? `Project ${project.title}` : folder ? folder : "Pick a folder to work in"}
      >
        <Icon name={project || !folder ? "project" : "folder"} size={12} />
        <span class="proj-title">{project ? project.title : folder ? folderName(folder) : "No project"}</span>
        {#if !folderLocked}<Icon name="chevDown" size={12} />{/if}
      </button>
      {#if folder && branch}
        <!-- Which branch: the folder as it is, or a worktree of its own. -->
        <button
          bind:this={branchBtn}
          class="proj branch-tab"
          class:folded={web}
          class:fresh={!!worktree}
          aria-hidden={web || undefined}
          tabindex={web ? -1 : undefined}
          aria-haspopup={folderLocked ? undefined : "menu"}
          aria-expanded={folderLocked ? undefined : branchOpen}
          on:click|stopPropagation={toggleBranch}
          use:tooltip={folderLocked ? `On ${branch}` : worktree ? `A worktree of its own, from ${worktree}` : `In the folder, on ${branch}`}
        >
          <Icon name="branch" size={12} />
          <span class="proj-branch">{worktree && !folderLocked ? `new from ${worktree}` : branch}</span>
          {#if !folderLocked}<Icon name="chevDown" size={12} />{/if}
        </button>
      {/if}
    </div>
  {/if}

  <div class="box" class:web role="group" aria-label="Composer" on:dragover|preventDefault on:drop|preventDefault={onDrop}>
    {#if webOpen}
      <div class="web-suggest" class:below={hero}>
        <SuggestList rows={webRows} active={webActive} typed={webTyped} on:pick={(e) => pickWeb(e.detail.row)} on:hover={(e) => (webActive = e.detail.index)} />
      </div>
    {/if}
    {#if slashItems.length || atItems.length}
      <div class="suggest" data-ui-list transition:fade={{ duration: 100 }}>
        {#each slashItems as cmd, i (cmd.name)}
          <button class:on={i === slashIndex} on:click={() => runSlash(cmd.name)} on:mousemove={() => (slashIndex = i)}>
            <span class="mono">/{cmd.name}</span><span class="hint">{cmd.hint}</span>
          </button>
        {/each}
        {#each atItems as item, i (item)}
          <button class="mono" class:on={i === atIndex} on:click={() => pickMention(item)}>{item}</button>
        {/each}
      </div>
    {/if}

    {#if held}
      <div class="held" transition:slide={{ duration: 160, easing: cubicOut }}>
        <!-- Waits for the run to finish; or now, or beside it, or back to edit. -->
        <span class="held-text" title={`${held}\n\nSends when the agent is done`}>{held}</span>
        <span class="held-acts">
          <button aria-label="Send now" use:tooltip={"Send now: stop the agent and send this"} on:click={() => dispatch("sendHeldNow")}><Icon name="arrowUp" size={12} /></button>
          <button aria-label="Side task" use:tooltip={"Side task: run it now, beside this session"} on:click={() => dispatch("sideHeld")}><Icon name="fork" size={12} /></button>
          <button aria-label="Edit" use:tooltip={"Edit"} on:click={() => dispatch("unhold")}><Icon name="pencil" size={12} /></button>
        </span>
      </div>
    {/if}

    {#if blind}
      <div class="held blind" transition:slide={{ duration: 160, easing: cubicOut }}>
        <Icon name="image" size={12} />
        <span class="held-text">{shownOf(model, board).name} can't see images</span>
        <button class="held-side" on:click={() => picker?.show()}>Change model</button>
      </div>
    {/if}

    {#if (attachments?.length ?? 0) || (quotes?.length ?? 0)}
      <div class="attachments" transition:slide={{ duration: 160, easing: cubicOut }}>
        {#each quotes ?? [] as q (`${q.project}/${q.id}`)}
          <span class="chip quote" transition:scale={{ duration: 140, start: 0.9 }}>
            <Icon name="plan" size={12} />
            <span class="chip-name">{q.title}</span>
            <button use:tooltip={`Remove ${q.title}`} on:click={() => (quotes = quotes.filter((x) => x !== q))}>
              <Icon name="close" size={12} />
            </button>
          </span>
        {/each}
        {#each attachments ?? [] as a (a)}
          <span class="chip" transition:scale={{ duration: 140, start: 0.9 }}>
            {#if thumbs[a]}
              <button class="thumb" aria-label="View {shortName(a)}" use:tooltip={"View"} on:click={() => viewImage(thumbs[a] ?? "", shortName(a))}>
                <img src={thumbs[a]} alt="" draggable="false" />
              </button>
            {:else}<Icon name="file" size={12} />{/if}
            <span class="chip-name" title={a}>{shortName(a)}</span>
            <button use:tooltip={`Remove ${a}`} on:click={() => (attachments = attachments.filter((x) => x !== a))}>
              <Icon name="close" size={12} />
            </button>
          </span>
        {/each}
      </div>
    {/if}

    <div class="row">
      {#if web}
        <span class="lead" aria-hidden="true"><Icon name="globe" /></span>
      {:else}
        <button class="lead btn" use:tooltip={"Attach files"} on:click={pickFiles}><Icon name="plus" /></button>
      {/if}
      <textarea
        bind:this={textarea}
        bind:value={input}
        rows="1"
        aria-label={web ? "Search or address" : "Message"}
        {placeholder}
        on:keydown={onKeydown}
        on:input={onInput}
        on:focus={() => (focused = true)}
        on:blur={() => (focused = false)}
        on:paste={onPaste}
      />
      {#if streaming}
        <button class="go stop" use:tooltip={"Stop (Esc)"} on:click={() => dispatch("stop")}><span class="square" /></button>
      {:else}
        <button class="go" class:ready={canSubmit} use:tooltip={web ? "Open (Enter)" : "Send (Enter)"} disabled={!canSubmit} on:click={submit}>
          <Icon name="arrowUp" />
        </button>
      {/if}
    </div>
  </div>

  <!-- Suggestions open under the box on Home: the line steps aside. -->
  <div class="under" class:hidden={webOpen && hero}>
    {#if !started}
      <div class="seg" role="group" aria-label="Send to" data-ui-segmented>
        <button class:on={!web} aria-pressed={!web} use:tooltip={"Ask the agent (Ctrl+1)"} on:click={() => setWeb(false)}>Agent</button>
        <button class:on={web} aria-pressed={web} use:tooltip={"Search the web (Ctrl+2)"} on:click={() => setWeb(true)}>Web</button>
      </div>
    {/if}
    {#if !web}
      {#if remoteHost}
        <button
          class="ctl"
          class:on={remote}
          disabled={remoteLocked}
          aria-pressed={remote}
          use:tooltip={remoteLocked ? "A session stays where it started" : remote ? "Run on this computer instead" : `Run on ${remoteHost} instead`}
          on:click|stopPropagation={() => dispatch("remote", { on: !remote })}
        >
          <Icon name={remote ? "server" : "desktop"} size={12} />
          <span class="truncate">{remote ? remoteHost : "This computer"}</span>
        </button>
      {/if}
      <button
        bind:this={permBtn}
        class="ctl"
        class:open={permOpen}
        class:warn={permission === "full"}
        use:tooltip={perm.desc}
        aria-haspopup="menu"
        aria-expanded={permOpen}
        on:click|stopPropagation={togglePerm}
      >
        <Icon name={permission === "supervised" ? "lock" : "unlock"} size={12} />
        <span>{perm.label}</span>
      </button>
      <span class="spacer" />
      <ModelPicker bind:this={picker} bind:value={model} bind:effort {board} on:unavailable />
    {/if}
  </div>
  {#if attachError}<div class="error" role="alert">{attachError}</div>{/if}

  {#if projOpen}
    <div class="menu-pop" data-ui-list style={projStyle} use:popover={{ anchor: projBtn, close: () => (projOpen = false) }} transition:fly={{ y: projStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each projects as p (p.slug)}
        <button class="opt" class:on={remote ? !!p.folder && p.folder === remoteCwd : project?.slug === p.slug} on:click={() => chooseProject(p)}>
          <Icon name="project" />
          <span class="name truncate">{p.title}</span>
          {#if !p.folder}<span class="count">No folder yet</span>{:else if !remote && sameDir(p.folder, folder) && branch}<span class="count">{branch}</span>{/if}
        </button>
      {:else}
        {#if projErr}
          <p class="pop-empty err">Couldn't load projects: {projErr}</p>
        {:else}
          <p class="pop-empty">No projects yet.</p>
        {/if}
      {/each}
      <div class="sep" />
      {#if remote}
        <button class="opt" on:click={newRemoteProject}><Icon name="plus" /><span class="name">New project…</span></button>
      {:else}
        {#if folder && !project}
          <button class="opt" on:click={makeProject}><Icon name="plus" /><span class="name truncate">Make {folderName(folder)} a project</span></button>
        {/if}
        <button class="opt" on:click={newProject}><Icon name="plus" /><span class="name">New project…</span></button>
      {/if}
      <button class="opt" class:on={remote ? !remoteCwd : !folder} on:click={() => chooseProject(null)}><Icon name="close" /><span class="name">No project</span></button>
      <button
        class="opt"
        on:click={() => {
          projOpen = false;
          dispatch("brain");
        }}
      >
        <Icon name="brain" /><span class="name">Manage projects</span>
      </button>
    </div>
  {/if}

  {#if branchOpen}
    <div class="menu-pop" data-ui-list style={branchStyle} use:popover={{ anchor: branchBtn, close: () => (branchOpen = false) }} transition:fly={{ y: branchStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      <button class="opt" class:on={!worktree} on:click={() => pickBranch("")}>
        <Icon name="folder" />
        <span class="name">This folder</span>
        <span class="count">{branch}</span>
      </button>
      <div class="sep" />
      <p class="pop-head">New worktree from</p>
      {#each branchList as b (b)}
        <button class="opt" class:on={worktree === b} on:click={() => pickBranch(b)}>
          <Icon name="branch" />
          <span class="name truncate">{b}</span>
        </button>
      {:else}
        <p class="pop-empty">No branches found.</p>
      {/each}
    </div>
  {/if}

  {#if permOpen}
    <div class="menu-pop perms" data-ui-list style={permStyle} use:popover={{ anchor: permBtn, close: () => (permOpen = false) }} transition:fly={{ y: permStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each PERMS as p (p.id)}
        {@const off = permissionBlocked(p.id, policy)}
        <button
          class="perm"
          disabled={!!off}
          on:click={() => {
            permission = p.id;
            permOpen = false;
          }}
        >
          <span class="meta"><span class="perm-name">{p.title}</span><span class="perm-sub">{off || p.desc}</span></span>
          {#if permission === p.id}<span class="tick"><Icon name="check" /></span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .ob {
    position: relative;
    width: 100%;
    min-width: 0;
    margin: 0 auto;
  }

  /* The project tab, fused to the box's top edge. */
  .proj {
    display: flex;
    align-items: center;
    gap: 6px;
    width: fit-content;
    max-width: calc(100% - 24px);
    height: 24px;
    margin: 0 0 0 14px;
    padding: 0 10px;
    transform: translateY(1px);
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
    transition:
      transform 220ms var(--ease-out),
      opacity 160ms ease;
  }
  .proj.folded {
    transform: translateY(100%);
    opacity: 0;
    pointer-events: none;
  }
  .proj:hover {
    color: var(--text);
  }
  .proj.set :global(svg:first-child) {
    color: var(--accent);
  }
  .proj-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
    color: var(--text);
  }
  .proj-branch {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 7%, transparent);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
  }

  .box {
    position: relative;
    z-index: 1;
    padding: 6px;
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-radius: var(--radius-lg);
    box-shadow: var(--glass-shadow);
  }
  .row {
    display: flex;
    align-items: flex-end;
    gap: 6px;
  }
  .lead {
    width: 30px;
    height: 30px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
  }
  .lead.btn {
    padding: 0;
    background: none;
    border: none;
    border-radius: 999px;
    cursor: pointer;
  }
  .lead.btn:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }
  textarea {
    flex: 1;
    min-width: 0;
    min-height: 30px;
    max-height: 180px;
    padding: 5px 2px;
    background: transparent;
    border: none;
    outline: none;
    resize: none;
    color: var(--text);
    font-size: var(--fs-base);
    line-height: 1.45;
  }
  textarea:focus {
    border-color: transparent !important;
    box-shadow: none !important;
  }
  textarea::placeholder {
    color: var(--muted);
  }
  .hero .box {
    padding: 8px;
  }
  .hero textarea {
    font-size: var(--fs-lg);
    padding: 4px 2px;
  }
  .go {
    width: 30px;
    height: 30px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: none;
    border-radius: 999px;
    color: var(--muted);
    cursor: pointer;
  }
  .go.ready:not(:disabled) {
    background: var(--text);
    color: var(--bg);
  }
  .go.ready:hover:not(:disabled) {
    opacity: 0.88;
  }
  .go:active:not(:disabled) {
    transform: scale(0.94);
    transition-duration: 60ms;
  }
  .go:disabled {
    cursor: default;
  }
  .go.stop {
    background: var(--line);
    color: var(--text);
  }
  .go.stop:hover {
    background: color-mix(in srgb, var(--bad) 22%, var(--bg));
    color: var(--bad);
  }
  .square {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: currentColor;
  }

  /* The line under the box: each control on its own small glass, like the
     project tab above it, so it reads on any wallpaper without joining the
     box. */
  .under {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    min-height: 32px;
    padding: 6px 0 0;
  }
  .under.hidden {
    visibility: hidden;
  }
  .seg,
  .under :global(.ctl) {
    box-sizing: border-box;
    height: 26px;
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-radius: var(--radius);
  }
  .seg {
    display: inline-flex;
    align-items: center;
    flex: none;
    gap: 2px;
    padding: 2px;
  }
  .seg button {
    height: 20px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--selected);
    color: var(--text);
    font-weight: 600;
  }
  .under :global(.ctl) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 0 8px;
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .under :global(.ctl:hover:not(:disabled)),
  .under :global(.ctl.open) {
    background: var(--panel);
    color: var(--text);
  }
  .under :global(.ctl:disabled) {
    cursor: default;
  }
  .ctl.warn {
    color: var(--warn);
  }
  .ctl.on :global(svg) {
    color: var(--accent);
  }
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .spacer {
    flex: 1 1 auto;
    min-width: 4px;
  }

  .held {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    margin: 2px 2px 6px;
    padding: 0 4px 0 10px;
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .held-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .held-acts {
    display: inline-flex;
    flex: none;
    gap: 8px;
  }
  .held .held-acts button {
    width: 22px;
    height: 22px;
  }
  .held .held-acts button:hover {
    background: var(--hover);
  }
  .held button {
    width: 18px;
    height: 18px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .held button.held-side {
    width: auto;
    padding: 0 6px;
    font: inherit;
    font-size: var(--fs-sm);
  }
  .held.blind > :global(svg) {
    flex: none;
    color: var(--warn);
  }
  .held button:hover {
    color: var(--text);
  }
  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 2px 2px 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    max-width: 220px;
    padding: 0 4px 0 8px;
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .chip img {
    width: 18px;
    height: 18px;
    object-fit: cover;
    border-radius: 3px;
  }
  /* The chip's own thumbnail opens the picture big. */
  .chip .thumb {
    display: inline-flex;
    padding: 0;
    background: none;
    border: none;
    border-radius: 3px;
    cursor: zoom-in;
  }
  .chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .chip button {
    width: 18px;
    height: 18px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .chip button:hover {
    color: var(--text);
  }
  .chip.quote {
    border-radius: 999px;
  }
  .error {
    padding: 4px 12px 0;
    font-size: var(--fs-xs);
    color: var(--bad);
  }
  .web-suggest {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 6;
  }
  .web-suggest.below {
    top: calc(100% + 6px);
    bottom: auto;
  }
  .suggest {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-height: 240px;
    padding: 5px;
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .suggest button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .suggest button.on,
  .suggest button:hover {
    background: var(--line);
    color: var(--text);
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--fs-sm);
  }
  .hint {
    margin-left: auto;
    font-size: var(--fs-xs);
    color: var(--muted);
  }

  .menu-pop {
    width: 260px;
    padding: 4px;
    overflow-y: auto;
  }
  .menu-pop.perms {
    width: 280px;
  }
  .perm {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 7px 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .perm:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .perm:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .perm-name {
    font-size: var(--fs-md);
  }
  .perm-sub {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .tick {
    display: inline-flex;
    color: var(--text);
  }
  .pop-empty {
    margin: 0;
    padding: 6px 8px 8px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .pop-empty.err {
    color: var(--bad);
  }
  .pop-head {
    margin: 0;
    padding: 4px 10px 2px;
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  /* The project and branch tabs sit side by side on the box's top edge. */
  .tops {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .proj.branch-tab {
    margin-left: 0;
  }
  .proj.branch-tab.fresh :global(svg:first-child) {
    color: var(--accent);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line);
  }
  .count {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover,
  .opt.on {
    background: var(--line);
    color: var(--text);
  }
  .opt.on :global(svg) {
    color: var(--accent);
  }
  .name {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-md);
  }
</style>
