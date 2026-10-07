<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly, scale, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import { api, brain, MODE_META, type ComposerMode, type Project, type ProviderStatus } from "./api";
  import { effortHint, effortLabel, effortsFor, fitEffort } from "./providerRows";
  import { folderName, toAddress } from "./tabs";
  import { popover, placeAbove } from "./popover";
  import { bookmarks, completeAddress, history, pins } from "./browserData";
  import SuggestList from "./SuggestList.svelte";
  import { boxText, firstRows, looksLikeUrl, mergeRows, pageRows, phraseRows, type Suggestion } from "./suggest";
  import type { IconName } from "./icons";

  export let input = "";
  export let model = "auto";
  export let effort = "medium";
  export let permission = "full";
  export let mode: ComposerMode = "build";
  export let attachments: string[] = [];
  export let folder = "";
  export let folderLocked = false;
  export let branch = "";
  export let streaming = false;
  export let board: ProviderStatus[] = [];
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let hero = false;
  export let project: { slug: string; title: string; tokens: number } | null = null;
  export let lockMode: ComposerMode | null = null;
  export let tokensIn = 0;
  export let tokensOut = 0;
  export let costUsd = 0;

  const dispatch = createEventDispatcher<{
    send: void;
    browse: { url: string };
    stop: void;
    command: { name: string };
    unavailable: { provider: string };
    folder: { path: string };
    project: void;
    brain: void;
  }>();

  const PERMS: { id: string; title: string; desc: string; icon: IconName }[] = [
    { id: "supervised", title: "Supervised", desc: "Ask before commands and file changes.", icon: "lock" },
    { id: "edits", title: "Auto-accept edits", desc: "Approve edits, ask before anything else.", icon: "pencil" },
    { id: "auto", title: "Auto", desc: "Agents that support it approve routine actions.", icon: "spark" },
    { id: "full", title: "Full access", desc: "Run commands and edits without asking.", icon: "unlock" },
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
  let effortBtn: HTMLButtonElement | null = null;
  let effortOpen = false;
  let effortStyle = "";
  let ctxBtn: HTMLButtonElement | null = null;
  let ctxOpen = false;
  let ctxStyle = "";
  let projBtn: HTMLButtonElement | null = null;
  let projOpen = false;
  let projStyle = "";
  let projects: Project[] = [];
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
  let focused = false;

  $: efforts = effortsFor(model, board);
  $: {
    const next = fitEffort(effort, efforts);
    if (next !== effort) effort = next;
  }
  $: perm = PERMS.find((p) => p.id === permission) ?? PERMS[3];
  $: contextPct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;
  $: slashQuery = /^\/\w*$/.test(input.trim()) ? input.trim().slice(1).toLowerCase() : null;
  $: slashItems = slashQuery === null ? [] : SLASH.filter((c) => c.name.startsWith(slashQuery));
  $: if (slashIndex >= slashItems.length) slashIndex = 0;
  $: void loadThumbs(attachments, folder);

  function kTokens(n: number) {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
    return String(n);
  }

  export function focus() {
    textarea?.focus();
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

  function toggleEffort() {
    effortOpen = !effortOpen;
    if (effortOpen && effortBtn) effortStyle = placeAbove(effortBtn, 240);
  }

  function toggleCtx() {
    ctxOpen = !ctxOpen;
    if (ctxOpen && ctxBtn) ctxStyle = placeAbove(ctxBtn, 260);
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
      return;
    }
    try {
      const found = await api.listFiles(folder, at[1].split("/").pop() ?? "");
      atItems = found.slice(0, 8);
      atIndex = 0;
    } catch {
      atItems = [];
    }
  }

  function pickMention(item: string) {
    input = input.replace(/@[\w./-]*$/, `@${item} `);
    addAttachments([item]);
    atItems = [];
    textarea?.focus();
  }

  function relative(path: string) {
    const root = folder.replace(/[/\\]+$/, "");
    if (root && (path.startsWith(`${root}\\`) || path.startsWith(`${root}/`))) {
      return path.slice(root.length + 1);
    }
    return path;
  }

  function addAttachments(paths: string[]) {
    const next = paths.map(relative).filter((p) => p && !attachments.includes(p));
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

  function sameDir(a: string, b: string) {
    const norm = (p: string) => p.replace(/[/\\]+$/, "").replace(/\\/g, "/").toLowerCase();
    return !!a && !!b && norm(a) === norm(b);
  }

  async function toggleProject() {
    if (folderLocked) return;
    projOpen = !projOpen;
    if (!projOpen || !projBtn) return;
    projStyle = placeAbove(projBtn, 340);
    projects = await brain.projects().catch(() => []);
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

  function shortName(name: string) {
    const base = name.split(/[/\\]/).pop() ?? name;
    return base.length > 22 ? `${base.slice(0, 22)}…` : base;
  }

  function togglePerm() {
    permOpen = !permOpen;
    if (permOpen && permBtn) permStyle = placeAbove(permBtn, 280);
  }
</script>

<div class="ob" class:hero>
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
          <Icon name="enter" size={15} />
        </button>
      {/if}
    </div>

    <div class="bar">
      <button class="ctl" title="Attach files" on:click={pickFiles}><Icon name="plus" size={14} stroke={2} /></button>
    <div class="modes" role="tablist" aria-label="Mode">
      {#if lockMode}
        <span class="mode on solo" style:--tint={MODE_META[lockMode].tint} title={MODE_META[lockMode].label}>
          <Icon name={MODE_META[lockMode].icon} size={13} />
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
              if (!locked) mode = m;
            }}
          >
            <Icon name={MODE_META[m].icon} size={13} />
            <span>{MODE_META[m].label}</span>
          </button>
        {/each}
      {/if}
    </div>
      {#if mode === "build"}
        <button bind:this={permBtn} class="ctl icon-only" class:open={permOpen} title={perm.title + " — " + perm.desc} on:click|stopPropagation={togglePerm}>
          <Icon name={perm.icon} size={12} />
        </button>
        {#if folder || project}
          <button
            bind:this={projBtn}
            class="ctl project"
            class:open={projOpen}
            class:set={!!project}
            class:locked={folderLocked}
            title={project ? `Project ${project.title} · ${folder} · about ${project.tokens} tokens of notes per session` : folder ? `${folder} · not a project yet` : "Pick the project this session works on"}
            on:click|stopPropagation={toggleProject}
          >
            <Icon name={project || !folder ? "project" : "folder"} size={12} />
            <span class="truncate">{project ? project.title : folder ? folderName(folder) : "No project"}</span>
            {#if branch}<span class="dim">{branch}</span>{/if}
            {#if !folderLocked}<Icon name="chevDown" size={10} stroke={2} />{/if}
          </button>
        {/if}
      {/if}

      <span class="spacer" />

      {#if mode !== "search"}
        <ModelPicker bind:this={picker} bind:value={model} {board} on:unavailable />
        <button
          bind:this={effortBtn}
          class="ctl"
          class:open={effortOpen}
          title={effortHint(effort) ? `Effort: ${effortHint(effort)}. Click to change.` : "Effort. Click to change."}
          on:click|stopPropagation={toggleEffort}
        >
          <span class="truncate">{effortLabel(effort)}</span>
          <Icon name="chevDown" size={10} stroke={2} />
        </button>
        {#if (mode === "build" || mode === "work") && contextLimit > 0 && (contextUsed > 0 || compacting)}
          <button
            bind:this={ctxBtn}
            class="ctx"
            class:warn={contextPct >= 70}
            class:bad={contextPct >= 90}
            disabled={compacting || streaming}
            title={compacting ? "Compacting…" : `Context ${contextPct}% full. Click for breakdown.`}
            on:click|stopPropagation={toggleCtx}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
              <circle class="track" cx="8" cy="8" r="6" />
              <circle class="fill" cx="8" cy="8" r="6" stroke-dasharray={RING} stroke-dashoffset={compacting ? RING * 0.7 : RING * (1 - contextPct / 100)} />
            </svg>
          </button>
        {/if}
      {/if}
    </div>
  </div>
  {#if attachError}<div class="error" role="alert">{attachError}</div>{/if}

  {#if effortOpen}
    <div class="menu-pop" style={effortStyle} use:popover={{ anchor: effortBtn, close: () => (effortOpen = false) }} transition:fly={{ y: effortStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      <div class="pop-head">Effort</div>
      {#each efforts as e (e)}
        <button
          class="opt"
          class:on={effort === e}
          on:click={() => {
            effort = e;
            effortOpen = false;
          }}
        >
          <span class="meta"><span class="name">{effortLabel(e)}</span><span class="sub">{effortHint(e) || "Default depth"}</span></span>
          {#if effort === e}<span class="tick"><Icon name="check" size={13} stroke={2} /></span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if ctxOpen}
    <div class="menu-pop ctx-pop" style={ctxStyle} use:popover={{ anchor: ctxBtn, close: () => (ctxOpen = false) }} transition:fly={{ y: ctxStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      <div class="pop-head">Context window · {contextPct}%</div>
      <div class="ctx-bar"><i style:width="{contextPct}%" /></div>
      <div class="ctx-rows">
        <div><span>Used</span><b>{kTokens(contextUsed)} of {kTokens(contextLimit)}</b></div>
        <div><span>Session</span><b>{kTokens(tokensIn)} in · {kTokens(tokensOut)} out</b></div>
        {#if costUsd > 0}<div><span>Cost</span><b>${costUsd.toFixed(4)}</b></div>{/if}
      </div>
      <button
        class="opt"
        aria-disabled={compacting || streaming}
        on:click={() => {
          if (compacting || streaming) return;
          ctxOpen = false;
          dispatch("command", { name: "compact" });
        }}
      >
        <span class="meta"><span class="name">{compacting ? "Compacting…" : "Compact now"}</span><span class="sub">Summarize to free context</span></span>
      </button>
    </div>
  {/if}

  {#if projOpen}
    <div class="menu-pop" style={projStyle} use:popover={{ anchor: projBtn, close: () => (projOpen = false) }} transition:fly={{ y: projStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      <div class="pop-head">Project</div>
      {#each projects as p (p.slug)}
        <button class="opt" class:on={project?.slug === p.slug} on:click={() => chooseProject(p)}>
          <Icon name="project" size={14} />
          <span class="meta"><span class="name">{p.title}</span><span class="sub truncate">{p.folder}</span></span>
          {#if p.notes.length}<span class="count">{p.notes.length} note{p.notes.length === 1 ? "" : "s"}</span>{/if}
        </button>
      {:else}
        <p class="pop-empty">No projects yet. A project is a folder plus the notes your agents should know about it.</p>
      {/each}
      <div class="sep" />
      {#if folder && !project}
        <button class="opt" on:click={makeProject}>
          <Icon name="plus" size={14} />
          <span class="meta"><span class="name">Make {folderName(folder)} a project</span><span class="sub">Notes you map to it come along to every session here</span></span>
        </button>
      {/if}
      <button class="opt" on:click={newProject}>
        <Icon name="plus" size={14} />
        <span class="meta"><span class="name">New project…</span><span class="sub">Pick the folder it lives in</span></span>
      </button>
      <button class="opt" class:on={!folder} on:click={() => chooseProject(null)}>
        <Icon name="close" size={14} />
        <span class="meta"><span class="name">No project</span><span class="sub">Run in an empty scratch folder</span></span>
      </button>
      <button
        class="opt"
        on:click={() => {
          projOpen = false;
          dispatch("brain");
        }}
      >
        <Icon name="brain" size={14} />
        <span class="meta"><span class="name">Manage projects in Brain</span></span>
      </button>
    </div>
  {/if}

  {#if permOpen}
    <div class="menu-pop perms" style={permStyle} use:popover={{ anchor: permBtn, close: () => (permOpen = false) }} transition:fly={{ y: permStyle.includes("bottom:") ? 6 : -6, duration: 140, easing: cubicOut }}>
      {#each PERMS as p (p.id)}
        <button
          class="perm"
          on:click={() => {
            permission = p.id;
            permOpen = false;
          }}
        >
          <span class="perm-icon"><Icon name={p.icon} size={13} /></span>
          <span class="meta"><span class="perm-name">{p.title}</span><span class="perm-sub">{p.desc}</span></span>
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
    width: 30px;
    height: 30px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
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
    width: 10px;
    height: 10px;
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
  .project.set {
    color: var(--text);
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
  .ctx {
    --ctx: var(--faint);
    width: 24px;
    height: 24px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    cursor: pointer;
  }
  .ctx:disabled {
    cursor: default;
  }
  .ctx.warn {
    --ctx: var(--warn);
  }
  .ctx.bad {
    --ctx: var(--bad);
  }
  .ctx svg {
    transform: rotate(-90deg);
  }
  .track {
    fill: none;
    stroke: var(--line);
    stroke-width: 2;
  }
  .fill {
    fill: none;
    stroke: var(--ctx);
    stroke-width: 2;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.4s ease;
  }
  .menu-pop {
    width: 340px;
    padding: 5px;
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
  .perm:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
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
  .ctx-pop {
    width: 260px;
    padding: 6px 10px 10px;
  }
  .ctx-bar {
    height: 6px;
    margin: 0 2px 10px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .ctx-bar i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .ctx-rows {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin: 0 2px 12px;
    font-size: 12px;
  }
  .ctx-rows div {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .ctx-rows span {
    color: var(--faint);
  }
  .ctx-rows b {
    font-weight: 550;
    color: var(--text);
    font-variant-numeric: tabular-nums;
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
    padding: 6px 8px;
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
  .opt[aria-disabled="true"] {
    opacity: 0.45;
    cursor: default;
  }
  .opt[aria-disabled="true"]:hover {
    background: transparent;
    color: var(--muted);
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
</style>
