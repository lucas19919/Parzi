<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly, scale, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import { api, brain, MODE_META, type ComposerMode, type Project, type ProviderStatus } from "./api";
  import { effortsFor, fitEffort } from "./providerRows";
  import { permissionBlocked } from "./lanes";
  import { folderName, toAddress } from "./tabs";
  import { relativeTo, sameDir, shortAttachment as shortName } from "./paths";
  import { popover, placeAbove } from "./popover";
  import { bookmarks, completeAddress, history, pins } from "./browserData";
  import SuggestList from "./SuggestList.svelte";
  import { boxText, firstRows, looksLikeUrl, mergeRows, pageRows, phraseRows, type Suggestion } from "./suggest";
  import type { IconName } from "./icons";

  export let input = "";
  export let model = "";
  export let effort = "medium";
  // Never defaults to full access: App seeds it from Settings' default.
  export let permission = "supervised";
  // Settings' default_mode; options it would ignore are shown disabled.
  export let policy = "";
  export let mode: ComposerMode = "build";
  export let attachments: string[] = [];
  export let folder = "";
  export let folderLocked = false;
  export let branch = "";
  export let streaming = false;
  export let board: ProviderStatus[] = [];
  export let hero = false;
  export let project: { slug: string; title: string; tokens: number } | null = null;
  export let lockMode: ComposerMode | null = null;
  // The linked server ("" when none). `remote` sends this composer's
  // sessions there; a started session stays where it began (`remoteLocked`).
  export let remoteHost = "";
  export let remote = false;
  export let remoteLocked = false;
  // Preview variants for live design iteration (Preview tab renders the
  // real component with variant 1-7). The live composer ships 6:
  // fused project tab on the top edge, project outside the box.
  export let variant = 2;
  $: outsideProject = variant === 5 || variant === 6 || variant === 7;

  const dispatch = createEventDispatcher<{
    send: void;
    browse: { url: string };
    stop: void;
    command: { name: string };
    unavailable: { provider: string };
    folder: { path: string };
    project: void;
    brain: void;
    remote: { on: boolean };
  }>();

  // ids are the `mode` strings the backend reads (ApprovalMode::parse:
  // "deny" blocks, "auto" and "full" run, anything else asks).
  const PERMS: { id: string; title: string; label: string; desc: string; icon: IconName }[] = [
    { id: "deny", title: "Read only", label: "Read only", desc: "Block commands and file changes.", icon: "shield" },
    { id: "supervised", title: "Supervised", label: "Ask", desc: "Ask before commands and file changes.", icon: "lock" },
    { id: "edits", title: "Auto-accept edits", label: "Edits", desc: "Approve edits, ask before anything else.", icon: "pencil" },
    { id: "auto", title: "Auto", label: "Auto", desc: "Agents that support it approve routine actions.", icon: "spark" },
    { id: "full", title: "Full access", label: "Full access", desc: "Run commands and edits without asking.", icon: "unlock" },
  ];

  const MODES: ComposerMode[] = ["search", "build", "work"];

  const SLASH = [
    { name: "search", hint: "search mode" },
    { name: "build", hint: "build mode" },
    { name: "work", hint: "work mode" },
    { name: "new", hint: "new session" },
    { name: "fork", hint: "branch this session" },
    { name: "compact", hint: "summarize to free context" },
    { name: "stop", hint: "stop the run" },
    { name: "model", hint: "pick a model" },
    { name: "effort", hint: "cycle effort" },
    { name: "page", hint: "open a web page" },
    { name: "settings", hint: "open settings" },
  ];

  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  const RING = 2 * Math.PI * 6;
  const MAX_ATTACH = 8;

  let textarea: HTMLTextAreaElement | null = null;
  let picker: ModelPicker;
  let permBtn: HTMLButtonElement | null = null;
  let permOpen = false;
  let permStyle = "";
  let projBtn: HTMLButtonElement | null = null;
  let projOpen = false;
  let projStyle = "";
  let projects: Project[] = [];
  let projErr = "";
  let atItems: string[] = [];
  let atIndex = 0;
  let slashIndex = 0;
  let thumbs: Record<string, string | null> = {};
  let attachError = "";
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
  $: perm = PERMS.find((p) => p.id === permission) ?? PERMS[1];
  $: slashQuery = /^\/\w*$/.test(input.trim()) ? input.trim().slice(1).toLowerCase() : null;
  $: slashItems = slashQuery === null ? [] : SLASH.filter((c) => c.name.startsWith(slashQuery));
  $: if (slashIndex >= slashItems.length) slashIndex = 0;
  // Attachments change rarely; guard by key so per-keystroke parent
  // object churn (same array, new dirty flag) doesn't re-decode images.
  $: {
    const key = `${folder}\n${attachments.join("\n")}`;
    if (key !== thumbsKey) {
      thumbsKey = key;
      void loadThumbs(attachments, folder);
    }
  }

  export function focus() {
    textarea?.focus();
  }

  export function openModels() {
    picker?.show();
  }

  function submit() {
    const text = input.trim();
    if (!text || streaming) return;
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
    else if ((MODES as string[]).includes(name)) {
      const next = name as ComposerMode;
      if (!lockMode || next === lockMode) mode = next;
    }
    else dispatch("command", { name });
  }

  function cycleEffort() {
    if (!efforts.length) return;
    effort = efforts[(Math.max(0, efforts.indexOf(effort)) + 1) % efforts.length];
  }

  function cycleMode() {
    if (lockMode) return;
    const order = MODES.filter((m) => lockMode === null || m === lockMode);
    mode = order[(order.indexOf(mode) + 1 + order.length) % order.length] ?? mode;
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
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (slashItems.length) runSlash(slashItems[slashIndex].name);
      else if (atItems.length) pickMention(atItems[atIndex]);
      else submit();
    } else if (e.key === "Tab" && !slashItems.length && !atItems.length) {
      e.preventDefault();
      cycleMode();
    } else if (/^[123]$/.test(e.key) && (e.ctrlKey || e.metaKey) && !slashItems.length && !atItems.length) {
      e.preventDefault();
      const next = MODES[Number(e.key) - 1];
      if (!lockMode || next === lockMode) mode = next;
    } else if (e.key === "Escape" && (slashItems.length || atItems.length)) {
      e.stopPropagation();
      atItems = [];
      input = input.replace(/^\/\w*$/, "");
    }
  }

  async function autosize() {
    await tick();
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  }

  $: if (lockMode && mode !== lockMode) mode = lockMode;
  $: if (textarea && !input) textarea.style.height = "";
  // Keystrokes call autosize via onInput; this covers programmatic fills
  // (edit-load, tab drafts) so the box always fits its text.
  $: if (textarea && input) void autosize();
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
    void autosize();
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
    const at = mode === "build" ? /@([\w./-]*)$/.exec(input) : null;
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
      projects = await brain.projects();
    } catch (e) {
      projects = [];
      projErr = String(e);
    }
  }

  function chooseProject(p: Project | null) {
    projOpen = false;
    dispatch("folder", { path: p ? p.folder : "" });
  }

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
    for (const f of images.slice(0, Math.max(0, MAX_ATTACH - attachments.length))) {
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

<div class="ob" class:hero class:v2={variant === 2} class:v3={variant === 3} class:v4={variant === 4} class:v5={variant === 5} class:v6={variant === 6} class:v7={variant === 7}>
  {#if outsideProject && remote}
    <span class="proj-out set remote-tab" title={`Runs on ${remoteHost}, in its own folder there`}>
      <Icon name="server" size={14} stroke={2} />
      <span class="proj-title">{remoteHost}</span>
    </span>
  {:else if outsideProject}
    <button bind:this={projBtn} class="proj-out" class:set={!!project} on:click|stopPropagation={toggleProject} title={project ? `Project ${project.title}${branch ? ` · ${branch}` : ""}` : "Pick the project this session works on"}>
      <Icon name={project || !folder ? "project" : "folder"} size={14} stroke={2} />
      <span class="proj-title">{project ? project.title : folder ? folderName(folder) : "No project"}</span>
      {#if branch}<span class="proj-branch">{branch}</span>{/if}
      {#if !folderLocked}<Icon name="chevDown" size={10} stroke={2} />{/if}
    </button>
  {/if}
  <div
    class="box"
    class:web={mode === "search"}
    role="group"
    aria-label="Composer"
    on:dragover|preventDefault
    on:drop|preventDefault={onDrop}
  >
    {#if webOpen}
      <div class="web-suggest" class:below={hero}>
        <SuggestList rows={webRows} active={webActive} typed={webTyped} on:pick={(e) => pickWeb(e.detail.row)} on:hover={(e) => (webActive = e.detail.index)} />
      </div>
    {/if}
    {#if slashItems.length || atItems.length}
      <div class="suggest" transition:fade={{ duration: 100 }}>
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

    {#if attachments.length}
      <div class="attachments" transition:slide={{ duration: 160, easing: cubicOut }}>
        {#each attachments as a (a)}
          <span class="chip" title={a} transition:scale={{ duration: 140, start: 0.9 }}>
            {#if thumbs[a]}<img src={thumbs[a]} alt="" draggable="false" />{:else}<Icon name="file" size={12} />{/if}
            <span class="chip-name">{shortName(a)}</span>
            <button aria-label="Remove {a}" on:click={() => (attachments = attachments.filter((x) => x !== a))}>
              <Icon name="close" size={10} stroke={2.2} />
            </button>
          </span>
        {/each}
      </div>
    {/if}

    <div class="row">
      {#if variant === 7}
        {#if project}<span class="inline-at">@{project.slug}</span>{/if}
      {/if}
      {#if streaming}
        <span class="orb" aria-hidden="true" title="Working…"><span class="eye" /><span class="eye" /></span>
      {/if}
      <textarea
        bind:this={textarea}
        bind:value={input}
        rows="1"
        placeholder={streaming ? "Working · Esc to stop" : MODE_META[mode].hint}
        on:keydown={onKeydown}
        on:input={onInput}
        on:focus={() => (focused = true)}
        on:blur={() => (focused = false)}
        on:paste={onPaste}
      />
      {#if streaming}
        <button class="go stop" title="Stop (Esc)" on:click={() => dispatch("stop")}><span class="square" /></button>
      {:else}
        <button class="go" class:ready={!!input.trim()} title={mode === "search" ? "Open (Enter)" : "Send (Enter)"} disabled={!input.trim()} on:click={submit}>
          <Icon name="arrowUp" size={14} stroke={2} />
        </button>
      {/if}
    </div>

    <div class="bar">
      <button class="ctl icon-only" title="Attach files" on:click={pickFiles}><Icon name="plus" size={14} stroke={2} /></button>
    <div class="modes" role="tablist" aria-label="Mode">
      {#if lockMode}
        <span class="mode on solo" style:--tint={MODE_META[lockMode].tint} title={MODE_META[lockMode].label}>
          <Icon name={MODE_META[lockMode].icon} size={14} stroke={2} />
        </span>
      {:else}
        {#each MODES as m, i (m)}
          {@const locked = lockMode !== null && m !== lockMode}
          <button
            role="tab"
            aria-selected={mode === m}
            class="mode"
            class:on={mode === m}
            class:off={locked}
            style:--tint={MODE_META[m].tint}
            title={locked ? `${MODE_META[m].label} (unavailable here)` : `${MODE_META[m].label} (Ctrl+${i + 1}, Tab cycles)`}
            disabled={locked}
            on:click={() => {
              if (locked) return;
              mode = m;
              if (m === "build") {
                effort = "medium";
              } else if (m === "work") {
                effort = "low";
              }
            }}
          >
            <Icon name={MODE_META[m].icon} size={14} stroke={2} />
            <span>{MODE_META[m].label}</span>
          </button>
        {/each}
      {/if}
    </div>
      {#if mode !== "search"}
        <!-- Every agent send carries this choice, so it shows for both lanes. -->
        <button
          bind:this={permBtn}
          class="ctl perm-btn"
          class:open={permOpen}
          class:full={permission === "full"}
          title={`${perm.title}: ${perm.desc}`}
          aria-haspopup="menu"
          aria-expanded={permOpen}
          on:click|stopPropagation={togglePerm}
        >
          <Icon name={perm.icon} size={13} stroke={2} />
          <span class="truncate">{perm.label}</span>
        </button>
      {/if}
      {#if remoteHost && mode !== "search"}
        <button
          class="ctl remote-btn"
          class:on={remote}
          disabled={remoteLocked}
          aria-pressed={remote}
          title={remoteLocked
            ? remote
              ? `This session runs on ${remoteHost}`
              : "This session runs on this PC"
            : remote
              ? `Runs on ${remoteHost}. Click to run on this PC`
              : `Run on ${remoteHost}`}
          on:click|stopPropagation={() => dispatch("remote", { on: !remote })}
        >
          <Icon name="server" size={13} stroke={2} />
          {#if remote}<span class="truncate">Remote</span>{/if}
        </button>
      {/if}
      {#if mode === "build" && !outsideProject}
          <button
            bind:this={projBtn}
            class="ctl project"
            class:open={projOpen}
            class:set={!!project}
            class:locked={folderLocked}
            title={project ? `Project ${project.title} · ${folder} · about ${project.tokens} tokens of notes per session` : folder ? `${folder} · not a project yet` : "Pick the project this session works on"}
            on:click|stopPropagation={toggleProject}
          >
            <Icon name={project || !folder ? "project" : "folder"} size={14} stroke={2} />
            <span class="truncate">{project ? project.title : folder ? folderName(folder) : "No project"}</span>
            {#if branch}<span class="dim">{branch}</span>{/if}
            {#if !folderLocked}<Icon name="chevDown" size={10} stroke={2} />{/if}
          </button>
      {/if}

      <span class="spacer" />

      {#if mode !== "search"}
        <ModelPicker bind:this={picker} bind:value={model} bind:effort {board} on:unavailable />
      {/if}
    </div>
  </div>
  {#if attachError}<div class="error" role="alert">{attachError}</div>{/if}

  {#if projOpen}
    <div class="menu-pop" style={projStyle} use:popover={{ anchor: projBtn, close: () => (projOpen = false) }} transition:fly={{ y: projStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      <div class="pop-head">Project</div>
      {#each projects as p (p.slug)}
        <button class="opt" class:on={project?.slug === p.slug} on:click={() => chooseProject(p)}>
          <Icon name="project" size={14} stroke={2} />
          <span class="meta"><span class="name">{p.title}</span><span class="sub truncate">{sameDir(p.folder, folder) && branch ? `${folderName(p.folder)} · ${branch}` : folderName(p.folder)}</span></span>
          {#if p.notes.length}<span class="count">{p.notes.length} note{p.notes.length === 1 ? "" : "s"}</span>{/if}
        </button>
      {:else}
        {#if projErr}
          <p class="pop-empty err">Couldn't load projects: {projErr}</p>
        {:else}
          <p class="pop-empty">No projects yet. A project is a folder plus the notes your agents should know about it.</p>
        {/if}
      {/each}
      <div class="sep" />
      {#if folder && !project}
        <button class="opt" on:click={makeProject}>
          <Icon name="plus" size={14} stroke={2} />
          <span class="meta"><span class="name">Make {folderName(folder)} a project</span><span class="sub">Notes you map to it come along to every session here</span></span>
        </button>
      {/if}
      <button class="opt" on:click={newProject}>
        <Icon name="plus" size={14} stroke={2} />
        <span class="meta"><span class="name">New project…</span><span class="sub">Pick the folder it lives in</span></span>
      </button>
      <button class="opt" class:on={!folder} on:click={() => chooseProject(null)}>
        <Icon name="close" size={14} stroke={2} />
        <span class="meta"><span class="name">No project</span><span class="sub">Run in an empty scratch folder</span></span>
      </button>
      <button
        class="opt"
        on:click={() => {
          projOpen = false;
          dispatch("brain");
        }}
      >
        <Icon name="brain" size={14} stroke={2} />
        <span class="meta"><span class="name">Manage projects in Brain</span></span>
      </button>
    </div>
  {/if}

  {#if permOpen}
    <div class="menu-pop perms" style={permStyle} use:popover={{ anchor: permBtn, close: () => (permOpen = false) }} transition:fly={{ y: permStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each PERMS as p (p.id)}
        {@const blocked = permissionBlocked(p.id, policy)}
        <button
          class="perm"
          disabled={!!blocked}
          title={blocked || undefined}
          on:click={() => {
            permission = p.id;
            permOpen = false;
          }}
        >
          <span class="perm-icon"><Icon name={p.icon} size={13} /></span>
          <span class="meta"><span class="perm-name">{p.title}</span><span class="perm-sub">{blocked || p.desc}</span></span>
          {#if permission === p.id}<span class="tick"><Icon name="check" size={13} stroke={2} /></span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .ob {
    position: relative;
    width: 100%;
    max-width: 720px;
    min-width: 0;
    margin: 0 auto;
  }
  .box {
    position: relative;
    padding: 6px 6px 6px 16px;
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-radius: var(--radius-lg);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.07),
      var(--glass-shadow);
    outline: none;
  }
  .box:focus,
  .box:focus-within {
    outline: none;
  }
  .row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .orb {
    width: 22px;
    height: 22px;
    flex: none;
    align-self: center;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--text) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--text) 16%, transparent);
    animation: orb-bob 2.4s ease-in-out infinite;
  }
  .eye {
    width: 3px;
    height: 4.5px;
    border-radius: 2px;
    background: var(--text);
    opacity: 0.85;
    animation: orb-blink 4.4s ease-in-out infinite;
    transform-origin: center;
  }
  @keyframes orb-bob {
    0%,
    100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(-1.5px);
    }
  }
  @keyframes orb-blink {
    0%,
    91%,
    100% {
      transform: scaleY(1);
    }
    94.5% {
      transform: scaleY(0.12);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .orb,
    .eye {
      animation: none;
    }
  }
  textarea {
    flex: 1;
    min-width: 0;
    min-height: 22px;
    max-height: 180px;
    padding: 6px 0;
    background: transparent;
    border: none;
    outline: none;
    resize: none;
    color: var(--text);
    font-size: 14px;
    line-height: 1.45;
  }
  textarea:focus {
    border-color: transparent !important;
    box-shadow: none !important;
  }
  textarea::placeholder {
    color: var(--faint);
  }
  .hero textarea {
    font-size: 15px;
    padding: 9px 0;
  }
  .go {
    width: 26px;
    height: 26px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: 1px solid transparent;
    border-radius: 999px;
    color: var(--faint);
    cursor: pointer;
  }
  .go.ready:not(:disabled) {
    background: color-mix(in srgb, var(--text) 14%, transparent);
    color: var(--text);
    box-shadow: none;
  }
  .go.ready:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text) 22%, transparent);
    transform: none;
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
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: currentColor;
  }
  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 0 2px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    max-width: 200px;
    padding: 0 4px 0 6px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12px;
  }
  .chip img {
    width: 18px;
    height: 18px;
    object-fit: cover;
    border-radius: 3px;
  }
  .chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 11px;
  }
  .chip button {
    width: 18px;
    height: 18px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--faint);
    cursor: pointer;
  }
  .chip button:hover {
    color: var(--text);
  }
  .error {
    padding: 4px 12px 0;
    font-size: 11px;
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
    font-size: 12.5px;
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
    font-size: 12px;
  }
  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--faint);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    margin-top: 6px;
    padding: 7px 6px 1px;
    border-top: 1px solid color-mix(in srgb, var(--text) 8%, transparent);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .bar::-webkit-scrollbar {
    display: none;
  }
  .bar :global(.ctl) {
    height: 26px;
    padding: 0 8px;
    color: var(--muted);
    font-size: 12px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: 1px solid transparent;
  }
  .bar :global(.ctl.icon-only) {
    padding: 0;
    width: 26px;
    justify-content: center;
  }
  .bar :global(.ctl:hover),
  .bar :global(.ctl.open) {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }
  .bar :global(.ctl:active) {
    transform: scale(0.97);
  }
  .perm-btn {
    flex: none;
    gap: 5px;
  }
  .perm-btn.full :global(svg) {
    color: var(--warn);
  }
  .remote-btn {
    flex: none;
    gap: 5px;
  }
  .remote-btn.on {
    color: var(--text);
  }
  .remote-btn.on :global(svg) {
    color: var(--accent);
  }
  .remote-btn:disabled {
    cursor: default;
  }
  .remote-tab {
    cursor: default;
  }
  .project.set {
    color: var(--text);
  }
  .project {
    flex: none;
    max-width: 220px;
  }
  .project .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .project.set :global(svg:first-child) {
    color: var(--accent);
  }
  .bar :global(.ctl.locked:hover) {
    background: transparent;
    transform: none;
  }
  .modes {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--text) 6%, transparent);
  }
  .mode {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 9px;
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--faint);
    font-size: 12px;
    cursor: pointer;
  }
  .mode:hover {
    color: var(--text);
  }
  .mode.on {
    background: color-mix(in srgb, var(--tint) 18%, transparent);
    color: var(--text);
  }
  .mode.on :global(svg) {
    color: var(--tint);
  }
  .mode.solo {
    cursor: default;
    padding: 0 8px;
  }
  .mode.off {
    opacity: 0.35;
    cursor: default;
  }
  .locked {
    cursor: default;
  }
  .dim {
    flex: none;
    white-space: nowrap;
    color: var(--faint);
    opacity: 0.8;
  }
  .spacer {
    flex: 1 1 auto;
    min-width: 4px;
  }
  .menu-pop {
    width: 260px;
    padding: 4px;
    overflow-y: auto;
  }
  .menu-pop.perms {
    width: 280px;
    padding: 4px;
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
  .perm-icon {
    display: inline-flex;
    color: var(--muted);
  }
  .perm-name {
    font-size: 13px;
  }
  .perm-sub {
    font-size: 11.5px;
    color: var(--faint);
  }
  .tick {
    display: inline-flex;
    color: var(--text);
  }
  .pop-head {
    padding: 6px 8px 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .pop-empty {
    margin: 0;
    padding: 6px 8px 8px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--muted);
  }
  .pop-empty.err {
    color: var(--bad);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line);
  }
  .count {
    flex: none;
    font-size: 11px;
    color: var(--faint);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 5px 8px;
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
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name {
    font-size: 12.5px;
    font-weight: 600;
  }
  .sub {
    font-size: 11px;
    line-height: 1.4;
    color: var(--muted);
  }
  /* Preview variants (see PreviewView): same controls, different layout. */
  /* Edition A — priority row: icon-only modes, project-first, one row. */
  .ob.v2 .mode span:last-child {
    display: none;
  }
  .ob.v2 .mode {
    padding: 0 7px;
  }
  .ob.v2 .bar :global(.model) {
    max-width: 200px;
  }
  .ob.v2 .project {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    color: var(--text);
  }
  /* Edition B — two-line calm: project on its own full-width row. */
  .ob.v3 .bar {
    flex-wrap: wrap;
  }
  .ob.v3 .project {
    order: -1;
    flex: 1 1 100%;
    justify-content: flex-start;
  }
  .ob.v3 .bar :global(.model) {
    max-width: 200px;
  }
  /* Edition C — compact: tighter pills, everything kept, nothing hidden. */  .ob.v4 .bar {
    flex-wrap: nowrap;
    gap: 3px;
  }
  .ob.v4 .mode {
    height: 22px;
    padding: 0 7px;
    font-size: 11.5px;
  }
  .ob.v4 .modes {
    padding: 1px;
  }
  .ob.v4 .bar :global(.ctl) {
    height: 24px;
    font-size: 11.5px;
  }
  .ob.v4 .bar :global(.ctl.icon-only) {
    width: 24px;
  }
  .ob.v4 .bar :global(.model) {
    max-width: 150px;
  }
  .ob.v4 .project {
    min-width: 0;
    max-width: 150px;
  }
  .ob.v4 .project .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Shared outside-project header (v5/v6/v7). */
  .proj-out {
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: 100%;
    margin: 0 0 6px 2px;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
  }
  .proj-out:hover { color: var(--text); }
  .proj-out.set :global(svg:first-child) { color: var(--accent); }
  .proj-title { font-weight: 650; color: var(--text); white-space: nowrap; }
  .proj-branch { flex: none; padding: 1px 7px; border-radius: 999px; background: color-mix(in srgb, var(--text) 7%, transparent); font-family: var(--mono); font-size: 10.5px; color: var(--muted); }
  /* v5: quiet eyebrow row, inline project hidden, box slimmer. */
  .ob.v5 .bar .project, .ob.v6 .bar .project, .ob.v7 .bar .project { display: none; }
  .ob.v5 .box { padding-top: 8px; }
  .ob.v5 .mode span:last-child { display: none; }
  .ob.v5 .mode { padding: 0 7px; }
  /* v6: tab fused to box top edge. */
  .ob.v6 .proj-out {
    margin: 0 0 0 10px;
    padding: 3px 10px;
    transform: translateY(1px);
    background: var(--glass-strong-bg);
    border: var(--glass-hairline);
    border-bottom: none;
    border-radius: 9px 9px 0 0;
    width: fit-content;
    max-width: calc(100% - 20px);
  }
  .ob.v6 .box { border-radius: 12px; }
  .ob.v6 .mode span:last-child { display: none; }
  .ob.v6 .mode { padding: 0 7px; }
  /* v7: innovative inline @mention, minimal bar, icons only. */
  .ob.v7 .proj-out { margin-bottom: 7px; }
  .ob.v7 .inline-at {
    flex: none;
    align-self: center;
    padding: 2px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--text);
    font-family: var(--mono);
    font-size: 11.5px;
    font-weight: 600;
  }
  .ob.v7 .mode span:last-child, .ob.v7 .bar :global(.model .lbl) { display: none; }
  .ob.v7 .mode { padding: 0 7px; }
  .ob.v7 .bar { border-top-style: dashed; }
</style>
