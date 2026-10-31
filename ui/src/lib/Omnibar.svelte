<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly, scale, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import { api, brain, type ComposerMode, type PlanQuote, type Project, type ProviderStatus } from "./api";
  import { remote as server } from "./remote";
  import { effortLabel, effortsFor, fitEffort, seesImages, shownOf } from "./providerRows";
  import { permissionBlocked } from "./permission";
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
  export let permission = "full";
  export let policy = "";
  export let mode: ComposerMode = "build";
  export let attachments: string[] = [];
  export let quotes: PlanQuote[] = [];
  export let folder = "";
  export let folderLocked = false;
  export let started = false;
  $: if (started && mode === "search") mode = "build";
  export let branch = "";
  export let streaming = false;
  export let queue: { id: string; text: string; detail?: string }[] = [];
  export let queuePaused = false;
  export let canSide = false;
  export let board: ProviderStatus[] = [];
  export let hero = false;
  export let project: { slug: string; title: string; tokens: number } | null = null;
  export let remoteHost = "";
  export let remote = false;
  export let remoteLocked = false;
  export let remoteCwd = "";

  const dispatch = createEventDispatcher<{
    send: void;
    unhold: { id: string };
    side: void;
    sideHeld: { id: string };
    sendHeldNow: { id: string };
    dropHeld: { id: string };
    moveHeld: { id: string; to: number };
    resumeQueue: void;
    browse: { url: string };
    stop: void;
    command: { name: string };
    unavailable: { provider: string };
    folder: { path: string };
    project: { path?: string };
    remote: { on: boolean };
    remoteFolder: { path: string };
  }>();

  const PERMS: { id: string; title: string; label: string; icon: IconName }[] = [
    { id: "supervised", title: "Ask", label: "Ask", icon: "lock" },
    { id: "auto", title: "Auto", label: "Auto", icon: "spark" },
    { id: "full", title: "Full", label: "Full", icon: "unlock" },
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
  let effortBtn: HTMLButtonElement | null = null;
  let effortOpen = false;
  let effortStyle = "";
  let projBtn: HTMLButtonElement | null = null;
  export let worktree = "";
  let branchBtn: HTMLButtonElement | null = null;
  let branchOpen = false;
  let branchStyle = "";
  let branchList: string[] = [];
  let expanded: string | null = null;
  let dragId: string | null = null;
  let dragOver = -1;
  function queueDrop(to: number) {
    if (dragId) dispatch("moveHeld", { id: dragId, to });
    dragId = null;
    dragOver = -1;
  }
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
  let delSlug: string | null = null;
  let atItems: string[] = [];
  let atIndex = 0;
  let slashIndex = 0;
  let thumbs: Record<string, string | null> = {};
  let attachError = "";
  let attachErrorTimer = 0;
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
  $: if (efforts.length < 2) effortOpen = false;
  $: perm = PERMS.find((p) => p.id === permission) ?? PERMS[0];
  $: slashQuery = /^\/\w*$/.test(input.trim()) ? input.trim().slice(1).toLowerCase() : null;
  $: slashItems =
    slashQuery === null ? [] : SLASH.filter((c) => c.name.startsWith(slashQuery) && !(started && (c.name === "web" || c.name === "agent")));
  $: if (slashIndex >= slashItems.length) slashIndex = 0;
  $: {
    const files = Array.isArray(attachments) ? attachments : [];
    const key = `${folder}\n${files.join("\n")}`;
    if (key !== thumbsKey) {
      thumbsKey = key;
      void loadThumbs(files, folder);
    }
  }
  $: if (!Array.isArray(quotes)) quotes = [];
  $: if (!Array.isArray(attachments)) attachments = [];
  $: if (!Array.isArray(queue)) queue = [];

  export function focus() {
    textarea?.focus();
  }

  export function openModels() {
    picker?.show();
  }

  $: web = mode === "search";
  $: placeholder = streaming
    ? "Queue a message for when it's done"
    : web
      ? "Search the web or type an address"
      : folder || remoteCwd
        ? "Describe a task or ask a question"
        : "Ask anything";
  $: blind = !web && !!model && (attachments ?? []).some((a) => IMAGE.test(a)) && !seesImages(model, board);
  $: canSubmit = !!input.trim() || ((quotes?.length ?? 0) > 0 && !web);

  function setWeb(on: boolean) {
    mode = on ? "search" : "build";
    if (on) {
      projOpen = false;
      permOpen = false;
      effortOpen = false;
    }
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
    } else if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && !started && !slashItems.length && !atItems.length) {
      e.preventDefault();
      setWeb(!web);
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

  async function autosize(text: string) {
    await tick();
    const el = textarea;
    if (!el) return;
    if (!text) {
      el.style.height = "";
      return;
    }
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, Math.round(window.innerHeight * 0.4))}px`;
  }

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
      projects = remote ? await server.projects() : await brain.projects();
    } catch (e) {
      projects = [];
      projErr = String(e);
    }
  }

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
      }
      dispatch("folder", { path });
      dispatch("project", { path });
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
    dispatch("project", { path: dir });
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

  async function deleteProject(p: Project) {
    if (delSlug !== p.slug) {
      delSlug = p.slug;
      return;
    }
    delSlug = null;
    try {
      await brain.remove(p.note);
      projects = await brain.projects();
      if (project?.slug === p.slug) dispatch("project", {});
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
    const data = e.clipboardData;
    if (!data) return;
    const types = [...data.types];
    const text = data.getData("text/plain");
    if (text.trim()) {
      console.info("paste", { types, inserted: "text" });
      return;
    }
    const files = data.files;
    if (!files?.length || ![...files].some((f) => f.type.startsWith("image/"))) {
      console.info("paste", { types, inserted: "none" });
      return;
    }
    console.info("paste", { types, inserted: "image" });
    e.preventDefault();
    void stageImages(files);
  }

  function onDrop(e: DragEvent) {
    const files = e.dataTransfer?.files;
    if (files?.length) void stageImages(files);
  }

  function togglePerm() {
    effortOpen = false;
    permOpen = !permOpen;
    if (permOpen && permBtn) permStyle = placeAbove(permBtn, 260);
  }

  function toggleEffort() {
    if (efforts.length < 2) return;
    permOpen = false;
    effortOpen = !effortOpen;
    if (effortOpen && effortBtn) effortStyle = placeAbove(effortBtn, 220);
  }
</script>

<div class="ob" class:hero>
  {#if queue.length}
    
    <div class="queue-bar" role="list" aria-label="Queued messages">
      {#if queuePaused}
        <div class="queue-head">
          <button class="queue-resume" on:click={() => dispatch("resumeQueue")}>Resume</button>
        </div>
      {/if}
      {#each queue as item, i (item.id)}
        <div
          class="queue-item"
          class:open={expanded === item.id}
          class:dragover={dragOver === i}
          role="listitem"
          transition:slide={{ duration: 160, easing: cubicOut }}
          on:dragover|preventDefault={() => (dragOver = i)}
          on:dragleave={() => (dragOver = -1)}
          on:drop|preventDefault={() => queueDrop(i)}
        >
          <span
            class="grip"
            draggable="true"
            role="button"
            tabindex="0"
          aria-label="Drag to reorder: {item.text}"
          use:tooltip={"Drag to reorder"}
            on:dragstart={() => (dragId = item.id)}
            on:dragend={() => ((dragId = null), (dragOver = -1))}
            on:keydown={(e) => {
              if (e.key === "ArrowUp" && i > 0) dispatch("moveHeld", { id: item.id, to: i - 1 });
              if (e.key === "ArrowDown" && i < queue.length - 1) dispatch("moveHeld", { id: item.id, to: i + 1 });
            }}
          ><Icon name="more" size={12} /></span>
          <span class="queue-num">{i + 1}</span>
          <button class="queue-text" class:open={expanded === item.id} title={item.text} on:click={() => (expanded = expanded === item.id ? null : item.id)}>
            <span>{item.text}</span>
            {#if item.detail}<span class="queue-detail">{item.detail}</span>{/if}
          </button>
          <span class="queue-acts">
            <button aria-label="Send now: {item.text}" use:tooltip={"Send now: stop the agent and send this"} on:click={() => dispatch("sendHeldNow", { id: item.id })}><Icon name="arrowUp" size={12} /></button>
            <button aria-label="Side task: {item.text}" use:tooltip={"Side task: run it now, beside this session"} on:click={() => dispatch("sideHeld", { id: item.id })}><Icon name="fork" size={12} /></button>
            <button aria-label="Edit: {item.text}" use:tooltip={"Edit"} on:click={() => dispatch("unhold", { id: item.id })}><Icon name="pencil" size={12} /></button>
            <button aria-label="Drop: {item.text}" use:tooltip={"Drop this message"} on:click={() => dispatch("dropHeld", { id: item.id })}><Icon name="close" size={12} /></button>
          </span>
        </div>
      {/each}
    </div>
  {/if}

  
  {#if !web && !started}
      {#if remote}
      <div class="where" transition:slide={{ duration: 160, easing: cubicOut }}>
        <span>Let's build {remoteProject ? "in" : "on"}</span>
        <button
          bind:this={projBtn}
          class="where-name"
          class:quiet={!remoteProject && !remoteHost}
          aria-haspopup={folderLocked ? undefined : "menu"}
          aria-expanded={folderLocked ? undefined : projOpen}
          on:click|stopPropagation={toggleProject}
          use:tooltip={remoteCwd ? `Runs on ${remoteHost} in ${remoteCwd}` : `Runs on ${remoteHost}, in its own scratch folder`}
        >{remoteProject || remoteHost || "a project"}</button>
        {#if remoteProject && remoteHost}<span>on {remoteHost}</span>{/if}
      </div>
    {:else}
      <div class="where" transition:slide={{ duration: 160, easing: cubicOut }}>
        <span>Let's build in</span>
        <button
          bind:this={projBtn}
          class="where-name"
          class:quiet={!project && !folder}
          aria-haspopup={folderLocked ? undefined : "menu"}
          aria-expanded={folderLocked ? undefined : projOpen}
          on:click|stopPropagation={toggleProject}
          use:tooltip={project ? `Project ${project.title}` : folder ? folder : "Pick a folder to work in"}
        >{project ? project.title : folder ? folderName(folder) : "a project"}</button>
        {#if folder && branch}
          <span>on</span>
          <button
            bind:this={branchBtn}
            class="where-branch"
            aria-haspopup={folderLocked ? undefined : "menu"}
            aria-expanded={folderLocked ? undefined : branchOpen}
            on:click|stopPropagation={toggleBranch}
            use:tooltip={folderLocked ? `On ${branch}` : worktree ? `A worktree of its own, from ${worktree}` : `In the folder, on ${branch}`}
          >{worktree && !folderLocked ? `new from ${worktree}` : branch}</button>
        {/if}
      </div>
    {/if}
  {/if}

  <div class="box" role="group" aria-label="Composer" on:dragover|preventDefault on:drop|preventDefault={onDrop}>
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
    
    <div class="tools">
      {#if !web}
        <ModelPicker bind:this={picker} bind:value={model} {board} on:unavailable />
        {#if efforts.length > 1}
          <button
            bind:this={effortBtn}
            class="text"
            class:open={effortOpen}
            use:tooltip={"Effort"}
            aria-haspopup="menu"
            aria-expanded={effortOpen}
            on:click|stopPropagation={toggleEffort}
          >{effortLabel(effort)}</button>
        {/if}
        <button
          bind:this={permBtn}
          class="text perm-btn"
          class:open={permOpen}
          aria-haspopup="menu"
          aria-expanded={permOpen}
          on:click|stopPropagation={togglePerm}
        >{perm.label}</button>
        {#if remoteHost}
          <button
            class="text"
            disabled={remoteLocked}
            aria-pressed={remote}
            use:tooltip={remoteLocked ? "A session stays where it started" : remote ? "Run on this computer instead" : `Run on ${remoteHost} instead`}
            on:click|stopPropagation={() => dispatch("remote", { on: !remote })}
          >
            <span class="truncate">{remote ? remoteHost : "This computer"}</span>
          </button>
        {/if}
      {/if}
      <div class="acts">
        {#if !web}
          <button class="icon" aria-label="Attach files" use:tooltip={"Attach files"} on:click={pickFiles}><Icon name="paperclip" /></button>
        {/if}
        {#if !started}
          <button
            class="icon"
            class:on={web}
            aria-pressed={web}
            aria-label="Browser"
            use:tooltip={web ? "Back to the agent" : "Browse the web"}
            on:click={() => setWeb(!web)}
          ><Icon name="globe" /></button>
        {/if}
        
        {#if streaming && !canSubmit}
          <button class="go stop" use:tooltip={"Stop"} on:click={() => dispatch("stop")}><span class="square" /></button>
        {:else}
          <button class="go" class:ready={canSubmit} use:tooltip={web ? "Open (Enter)" : "Send (Enter)"} disabled={!canSubmit} on:click={submit}>
            <Icon name="arrowUp" />
          </button>
        {/if}
      </div>
    </div>
  </div>

  {#if attachError}<div class="error" role="alert">{attachError}</div>{/if}

  {#if projOpen}
    <div class="menu-pop" data-ui-list style={projStyle} use:popover={{ anchor: projBtn, close: () => (projOpen = false) }} transition:fly={{ y: projStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each projects as p (p.slug)}
        <div class="opt-row" class:on={remote ? !!p.folder && p.folder === remoteCwd : project?.slug === p.slug}>
          <button class="opt grow" on:click={() => chooseProject(p)}>
            <Icon name="project" />
            <span class="name truncate">{p.title}</span>
            {#if !p.folder}<span class="count">No folder yet</span>{:else if !remote && sameDir(p.folder, folder) && branch}<span class="count">{branch}</span>{/if}
          </button>
          {#if !remote}
            <button
              class="del"
              class:armed={delSlug === p.slug}
              use:tooltip={delSlug === p.slug ? "Click again to delete" : `Delete ${p.title}`}
              aria-label={delSlug === p.slug ? `Confirm delete ${p.title}` : `Delete ${p.title}`}
              on:click={() => deleteProject(p)}
            >{#if delSlug === p.slug}Sure?{:else}<Icon name="trash" size={12} />{/if}</button>
          {/if}
        </div>
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

  {#if effortOpen}
    <div class="menu-pop" data-ui-list style={effortStyle} use:popover={{ anchor: effortBtn, close: () => (effortOpen = false) }} transition:fly={{ y: effortStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each efforts as level (level)}
        <button
          class="opt"
          class:on={effort === level}
          on:click={() => {
            effort = level;
            effortOpen = false;
          }}
        >
          <span class="name">{effortLabel(level)}</span>
          {#if effort === level}<span class="tick"><Icon name="check" /></span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if permOpen}
    <div class="menu-pop" data-ui-list style={permStyle} use:popover={{ anchor: permBtn, close: () => (permOpen = false) }} transition:fly={{ y: permStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each PERMS as p (p.id)}
        {@const off = permissionBlocked(p.id, policy)}
        <button
          class="opt"
          class:on={permission === p.id}
          disabled={!!off}
          title={off || p.title}
          on:click={() => {
            permission = p.id;
            permOpen = false;
          }}
        >
          <span class="name">{p.title}</span>
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

  .where {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space);
    min-width: 0;
    margin: 0 0 calc(var(--space) * 2) calc(var(--space) * 3);
    font-size: var(--fs-sm);
    line-height: 1.4;
    color: var(--muted);
  }
  .where-name,
  .where-branch {
    min-width: 0;
    max-width: 100%;
    padding: 0;
    overflow: hidden;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    font-weight: 600;
    line-height: inherit;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .where-name.quiet {
    color: var(--muted);
  }
  .where-name:hover,
  .where-branch:hover,
  .where-name.quiet:hover {
    color: var(--text);
  }

  .box {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 3);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: none;
  }
  textarea {
    width: 100%;
    min-width: 0;
    min-height: calc(var(--space) * 6);
    max-height: 40vh;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
    resize: none;
    color: var(--text);
    font-size: var(--fs-base);
    line-height: 1.45;
    user-select: text;
  }
  textarea:focus {
    border-color: transparent !important;
    box-shadow: none !important;
  }
  textarea::placeholder {
    color: var(--muted);
  }

  .go {
    width: calc(var(--space) * 6);
    height: calc(var(--space) * 6);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .go.ready:not(:disabled) {
    background: var(--accent);
    border-radius: 50%;
    color: var(--bg);
  }
  .go:active:not(:disabled) {
    transform: scale(0.94);
    transition-duration: var(--dur-fast);
  }
  .go:disabled {
    cursor: default;
  }
  .go.stop {
    color: var(--text);
  }
  .go.stop:hover {
    color: var(--bad);
  }
  .square {
    width: 9px;
    height: 9px;
    border-radius: var(--radius-sm);
    background: currentColor;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-width: 0;
  }
  .tools .text,
  .tools :global(.model) {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: calc(var(--space) * 6);
    max-width: 200px;
    min-width: 0;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    line-height: 1.45;
    cursor: pointer;
  }
  .tools .text:hover:not(:disabled),
  .tools .text.open,
  .tools :global(.model:hover),
  .tools :global(.model.open) {
    color: var(--text);
  }
  .tools .text:disabled {
    cursor: default;
  }
  .tools :global(.model .ver) {
    font-size: inherit;
  }
  .acts {
    display: inline-flex;
    align-items: center;
    flex: none;
    gap: calc(var(--space) * 2);
    margin-left: auto;
  }
  .icon {
    width: calc(var(--space) * 6);
    height: calc(var(--space) * 6);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover,
  .icon.on {
    color: var(--text);
  }
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }



  .queue-bar {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    max-height: 220px;
    margin: 0 0 calc(var(--space) * 2);
    overflow-y: auto;
  }
  .queue-head {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-height: 28px;
    padding: 0 var(--space);
  }
  .queue-resume {
    padding: var(--space) calc(var(--space) * 3);
    background: var(--text);
    border: none;
    border-radius: var(--radius-sm);
    color: var(--bg);
    font-size: var(--fs-xs);
    font-weight: 600;
    cursor: pointer;
  }
  .queue-item {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 36px;
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .queue-item.dragover {
    border-color: var(--accent);
  }
  .grip {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: grab;
  }
  .grip:active {
    cursor: grabbing;
  }
  .queue-num {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    min-width: 16px;
    height: 22px;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    line-height: 22px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .queue-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--space);
    padding: 0;
    background: none;
    border: none;
    color: var(--text);
    font-size: var(--fs-sm);
    line-height: 22px;
    text-align: left;
    cursor: pointer;
  }
  .queue-text > span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 22px;
  }
  .queue-text.open > span:first-child {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    text-overflow: unset;
    line-height: 1.4;
  }
  .queue-detail {
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .queue-acts {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  .queue-acts button {
    width: 22px;
    height: 22px;
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
  .queue-acts button:hover {
    background: var(--hover);
    color: var(--text);
  }
  .held {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: 26px;
    flex: none;
    margin: 0;
    padding: 0 var(--space) 0 calc(var(--space) * 3);
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
    padding: 0 calc(var(--space) * 2);
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
    gap: calc(var(--space) * 2);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: 26px;
    max-width: 220px;
    padding: 0 var(--space) 0 calc(var(--space) * 2);
    background: color-mix(in srgb, var(--text) 5%, transparent);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .chip img {
    width: 18px;
    height: 18px;
    object-fit: cover;
    border-radius: var(--radius-sm);
  }
  .chip .thumb {
    display: inline-flex;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
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
    border-radius: var(--radius);
  }
  .error {
    padding: var(--space) calc(var(--space) * 3) 0;
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
    gap: var(--space);
    max-height: 240px;
    padding: var(--space);
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .suggest button {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 2) calc(var(--space) * 2);
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
    background: var(--hover);
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
    padding: var(--space);
    overflow-y: auto;
  }
  .tick {
    display: inline-flex;
    color: var(--text);
  }
  .pop-empty {
    margin: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 2) calc(var(--space) * 2);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .pop-empty.err {
    color: var(--bad);
  }
  .pop-head {
    margin: 0;
    padding: var(--space) calc(var(--space) * 3) var(--space);
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  .sep {
    height: 1px;
    margin: var(--space) calc(var(--space) * 2);
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
    gap: calc(var(--space) * 2);
    width: 100%;
    height: 32px;
    padding: 0 calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover:not(.on) {
    background: var(--hover);
  }
  .opt.on {
    background: var(--selected);
  }
  .opt.on :global(svg) {
    color: var(--accent);
  }
  .opt:disabled {
    opacity: var(--disabled);
    cursor: default;
  }
  .opt-row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .opt-row:hover:not(.on) {
    background: var(--hover);
  }
  .opt-row.on {
    background: var(--selected);
  }
  .opt-row .opt.grow {
    flex: 1;
    min-width: 0;
    width: auto;
  }
  .opt-row:hover:not(.on) .opt.grow,
  .opt-row.on .opt.grow {
    background: transparent;
  }
  .opt-row .del {
    flex: none;
    min-width: 20px;
    height: 20px;
    margin-right: var(--space);
    padding: 0 var(--space);
    display: none;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-xs);
    cursor: pointer;
  }
  .opt-row:hover .del,
  .opt-row .del:focus-visible,
  .opt-row .del.armed {
    display: inline-flex;
  }
  .opt-row .del:hover {
    color: var(--text);
  }
  .opt-row .del.armed {
    color: var(--bad);
    font-weight: 600;
  }
  .name {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-md);
  }
</style>
