<script lang="ts">
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { createEventDispatcher } from "svelte";
  import { scale, fade, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { portal } from "./portal";
  import { api, hub, type Project, type ProviderStatus } from "./api";
  import { ageOf, checking, refreshBoard } from "./providerStore";
  import {
    AUTO_ROW, PROVIDER_ORDER, allRows, effortHint, effortLabel as effortWord, effortsFor,
    fitEffort, isUsable, nameOf, rowSub, shownOf, type PickRow,
  } from "./providerRows";

  export let input = "";
  export let model = "";
  export let mode: "agent" | "web" = "agent";
  /** Parzi's pill (low…ultra) or the chosen model's own word for it. */
  export let effort = "medium";
  export let streaming = false;
  export let currentProject = "default";
  export let branch = "";
  export let tokens = 0;
  export let board: ProviderStatus[] = [];
  export let projectRoot = "";
  export let attachments: string[] = [];
  export let permission: string = "full";
  export let workspaces: string[] = [];
  export let workspace = "";
  export let workspaceFixed = false;
  /** Deck projects of the workspace; a project lives in a workspace. */
  export let deckProjects: Project[] = [];
  export let selectedProject: { workspace: string; slug: string } | null = null;
  /** Context window fill of this chat (tokens the model saw last request). */
  export let contextUsed = 0;
  /** The model's window; 0 hides the meter. */
  export let contextLimit = 0;
  /** A compaction is running: the meter spins and does not click. */
  export let compacting = false;

  $: contextPct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;
  const RING = 2 * Math.PI * 6;
  function kTokens(n: number): string {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
    return String(n);
  }

  function toggleMode() {
    mode = mode === "agent" ? "web" : "agent";
    dispatch("modeChange", { mode });
  }

  function resolveWebUrl(raw: string): string {
    let t = raw.trim();
    if (!t) return "https://duckduckgo.com";
    if (/^https?:\/\//i.test(t)) return t;
    if (t.startsWith("localhost")) return "http://" + t;
    if (t.includes(".") && !t.includes(" ")) return "https://" + t;
    return `https://duckduckgo.com/?q=${encodeURIComponent(t)}`;
  }

  function submitAction() {
    if (streaming) return;
    const text = input.trim();
    if (!text) return;
    if (mode === "web") {
      dispatch("browse", { url: resolveWebUrl(text) });
      input = "";
    } else {
      dispatch("send");
    }
  }

  const dispatch = createEventDispatcher<{
    send: void;
    browse: { url: string };
    modeChange: { mode: "agent" | "web" };
    stop: void;
    modelChange: { model: string };
    command: { name: string; arg: string };
    permissionChange: { permission: string };
    workspaceChange: { workspace: string };
    workspaceDeleted: { workspace: string };
    openProject: { workspace: string; slug: string };
    error: { text: string };
    newWorkspace: void;
  }>();

  const I = {
    plus: "M12 5v14M5 12h14",
    sendUp: "M12 19V5M5 12l7-7 7 7",
    stop: "M7 7h10v10H7z",
    chevD: "M6 9l6 6 6-6",
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    check: "M20 6L9 17l-5-5",
    lock: "M7 11V7a5 5 0 0 1 10 0v4M5 11h14v10H5zM12 15v3",
    unlock: "M7 11V7a5 5 0 0 1 9.9-1M5 11h14v10H5zM12 15v3",
    pencil: "M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    spark: "M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4Z",
    clip: "M21 11.5l-8.5 8.5a5.5 5.5 0 0 1-7.8-7.8l8-8a3.7 3.7 0 0 1 5.2 5.2l-8 8a1.8 1.8 0 0 1-2.6-2.6l7-7",
    model: "M4 4h16v16H4z",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    file: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6",
    close: "M18 6L6 18M6 6l12 12",
  };

  const PERMS = [
    { id: "supervised", title: "Supervised", desc: "Ask before commands and file changes.", icon: I.lock },
    { id: "edits", title: "Auto-accept edits", desc: "Auto-approve edits, ask before other actions.", icon: I.pencil },
    { id: "auto", title: "Auto", desc: "Supported providers approve routine actions; others still ask.", icon: I.spark },
    { id: "full", title: "Full access", desc: "Allow commands and edits without prompts.", icon: I.unlock },
  ];

  let textareaEl: HTMLTextAreaElement | null = null;
  let showModelPicker = false;
  /** Left-rail selection: a roster provider id, or "__fav" for starred. */
  let railSel = "";
  let permOpen = false;
  let wsOpen = false;
  let projOpen = false;
  let modelQuery = "";
  let searchInputEl: HTMLInputElement | null = null;
  let modelIndex = 0;
  let modelBtn: HTMLButtonElement | null = null;
  let permBtn: HTMLButtonElement | null = null;
  let wsBtn: HTMLButtonElement | null = null;
  let projBtn: HTMLButtonElement | null = null;
  let wsPopStyle = "";
  let wsBelow = false;
  let projPopStyle = "";
  let projBelow = false;
  let modelPopStyle = "";
  let permPopStyle = "";
  let modelBelow = false;
  let permBelow = false;
  let atOpen = false;
  let atItems: string[] = [];
  let atIndex = 0;
  let slashOpen = false;
  let slashIndex = 0;

  /** An agent asked less than this long ago is not asked again on open. */
  const FRESH_SECS = 300;

  $: rows = allRows(board);
  $: q = modelQuery.trim().toLowerCase();

  function matches(row: PickRow): boolean {
    if (!q) return true;
    return (
      row.label.toLowerCase().includes(q) ||
      row.value.toLowerCase().includes(q) ||
      nameOf(row.provider).toLowerCase().includes(q)
    );
  }

  $: mainItems = ((): PickRow[] => {
    if (q) return [AUTO_ROW, ...rows].filter(matches);
    if (railSel === "__auto") return [AUTO_ROW];
    if (railSel === "__fav") return [...favRows];
    if (!railSel) return [];
    return rows.filter((r) => r.provider === railSel);
  })();
  $: if (modelIndex >= mainItems.length && mainItems.length) modelIndex = 0;

  $: shown = shownOf(model, board);
  $: railStatus = board.find((b) => b.provider === railSel);

  let favorites: string[] = [];
  $: favRows = favorites
    .map((v) => rows.find((r) => r.value === v))
    .filter((r): r is PickRow => !!r);

  /** What the chosen model takes, in its agent's words; empty = no knob. */
  $: efforts = effortsFor(model, board);
  $: fitTo(efforts);
  $: effortIdx = Math.max(0, efforts.indexOf(effort));
  $: permTitle = PERMS.find((p) => p.id === permission)?.title ?? "Full access";
  /** Projects of the open workspace, and the selected one's title. */
  $: wsDeck = deckProjects.filter((p) => !workspace || p.workspace === workspace);
  $: selDeck = selectedProject
    ? deckProjects.find((p) => p.workspace === selectedProject.workspace && p.slug === selectedProject.slug) ?? null
    : null;
  $: projLabel = selDeck ? selDeck.title || selDeck.slug : "No project";

  /** Keep the effort when the new model takes it; otherwise the closest fit. */
  function fitTo(list: string[]) {
    const next = fitEffort(effort, list);
    if (next !== effort) effort = next;
  }

  function setEffort(id: string) {
    effort = id;
  }

  function setPermission(id: string) {
    permission = id;
    permOpen = false;
    dispatch("permissionChange", { permission: id });
  }

  async function toggleFav(value: string) {
    try {
      favorites = await api.toggleFavorite(value);
    } catch {}
  }
  function selectModel(id: string) {
    model = id;
    showModelPicker = false;
    dispatch("modelChange", { model: id });
  }

  /** An agent that cannot start a turn opens its Settings card instead. */
  function pickRow(row: PickRow) {
    if (!row.usable) {
      dispatch("command", { name: "keys", arg: row.provider });
      showModelPicker = false;
      return;
    }
    selectModel(row.value);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" && (e.target as HTMLInputElement)?.placeholder?.includes("Search models")) {
        return; // let model search input handle its own Enter
      }
      e.preventDefault();
      if (slashOpen && slashItems.length) {
        runSlash(slashItems[slashIndex] ?? slashItems[0]);
      } else if (atOpen && atItems.length) {
        pickAt(atItems[atIndex] ?? atItems[0]);
      } else if (!streaming && input.trim()) {
        submitAction();
      }
    } else if (e.key === "Tab" && !slashOpen && !atOpen && !showModelPicker && !permOpen && !wsOpen && !projOpen) {
      e.preventDefault();
      toggleMode();
    } else if (e.key === "Escape") {
      if (showModelPicker) showModelPicker = false;
      else if (permOpen) permOpen = false;
      else if (wsOpen) wsOpen = false;
      else if (projOpen) projOpen = false;
      else if (slashOpen) slashOpen = false;
      else if (atOpen) atOpen = false;
    } else if (slashOpen && e.key === "ArrowDown") {
      e.preventDefault();
      slashIndex = (slashIndex + 1) % Math.max(1, slashItems.length);
    } else if (slashOpen && e.key === "ArrowUp") {
      e.preventDefault();
      slashIndex = (slashIndex - 1 + Math.max(1, slashItems.length)) % Math.max(1, slashItems.length);
    } else if (atOpen && e.key === "ArrowDown") {
      e.preventDefault();
      atIndex = (atIndex + 1) % atItems.length;
    } else if (atOpen && e.key === "ArrowUp") {
      e.preventDefault();
      atIndex = (atIndex - 1 + atItems.length) % atItems.length;
    }
  }

  async function handleInput() {
    if (textareaEl) {
      textareaEl.style.height = "auto";
      textareaEl.style.height = Math.min(textareaEl.scrollHeight, 180) + "px";
    }
    const at = input.match(/@([\w./-]*)$/);
    if (at && projectRoot) {
      try {
        atItems = await api.listFiles(projectRoot, at[1].split("/").slice(-1)[0]);
        atIndex = 0;
        atOpen = atItems.length > 0;
      } catch {
        atOpen = false;
      }
    } else {
      atOpen = false;
    }
    slashOpen = /^\/\w*$/.test(input.trim());
    slashIndex = 0;
  }

  function pickAt(item: string) {
    input = input.replace(/@[\w./-]*$/, `@${item} `);
    if (!attachments.includes(item)) attachments = [...attachments, item];
    atOpen = false;
    textareaEl?.focus();
    handleInput();
  }

  interface SlashCmd {
    name: string;
    hint: string;
    run: () => void;
  }

  $: slashItems = (() => {
    const q = input.trim().slice(1).toLowerCase();
    const all: SlashCmd[] = [
      { name: "/plan", hint: "task planner", run: () => dispatch("command", { name: "plan", arg: "" }) },
      { name: "/auto", hint: "smart auto routing", run: () => dispatch("command", { name: "auto", arg: "" }) },
      { name: "/model", hint: "open model picker", run: () => { openModelPicker(); } },
      { name: "/effort", hint: "cycle effort", run: () => dispatch("command", { name: "effort", arg: "" }) },
      { name: "/new", hint: "new thread", run: () => dispatch("command", { name: "new", arg: "" }) },
      { name: "/clear", hint: "clean thread, same lane", run: () => dispatch("command", { name: "clear", arg: "" }) },
      { name: "/compact", hint: "summarize to free context", run: () => dispatch("command", { name: "compact", arg: "" }) },
      { name: "/fork", hint: "branch this thread", run: () => dispatch("command", { name: "fork", arg: "" }) },
      { name: "/subsession", hint: "spawn a child subsession", run: () => dispatch("command", { name: "subsession", arg: "" }) },
      { name: "/kill", hint: "stop the run", run: () => dispatch("command", { name: "kill", arg: "" }) },
      { name: "/doctor", hint: "health checks", run: () => dispatch("command", { name: "doctor", arg: "" }) },
      { name: "/help", hint: "all commands", run: () => dispatch("command", { name: "help", arg: "" }) },
    ];
    return all.filter((c) => c.name.slice(1).startsWith(q));
  })();

  function runSlash(cmd: SlashCmd) {
    input = "";
    slashOpen = false;
    handleInput();
    cmd.run();
  }

  const POP_W = { model: 440, perm: 320, ws: 240, proj: 240 };

  function placePop(btn: HTMLButtonElement | null, which: "model" | "perm" | "ws" | "proj") {
    if (!btn || typeof window === "undefined") return;
    const r = btn.getBoundingClientRect();
    const w = POP_W[which];
    const left = Math.max(8, Math.min(which === "perm" || which === "ws" ? r.right - w : r.left, window.innerWidth - w - 8));
    const spaceAbove = r.top - 46;
    const spaceBelow = window.innerHeight - r.bottom - 8;
    let style: string;
    let below: boolean;
    if (spaceAbove >= 300 || spaceAbove >= spaceBelow) {
      const maxH = Math.max(180, Math.min(spaceAbove, 560));
      const bottom = Math.max(8, window.innerHeight - r.top + 8);
      style = `left:${Math.round(left)}px;bottom:${Math.round(bottom)}px;max-height:${Math.round(maxH)}px;`;
      below = false;
    } else {
      const maxH = Math.max(180, spaceBelow);
      style = `left:${Math.round(left)}px;top:${Math.round(r.bottom + 8)}px;max-height:${Math.round(maxH)}px;`;
      below = true;
    }
    if (which === "model") {
      modelPopStyle = style;
      modelBelow = below;
    } else if (which === "ws") {
      wsPopStyle = style;
      wsBelow = below;
    } else if (which === "proj") {
      projPopStyle = style;
      projBelow = below;
    } else {
      permPopStyle = style;
      permBelow = below;
    }
  }

  function repositionPops() {
    if (showModelPicker) placePop(modelBtn, "model");
    if (permOpen) placePop(permBtn, "perm");
    if (wsOpen) placePop(wsBtn, "ws");
    if (projOpen) placePop(projBtn, "proj");
  }

  function closeInlinePops() {
    slashOpen = false;
    atOpen = false;
  }

  /** Roster provider behind the current model value, if any. */
  function providerOfModel(m: string): string | null {
    const [p] = m.split("/");
    if (m === "auto" || !p) return null;
    return PROVIDER_ORDER.includes(p) && board.some((r) => r.provider === p) ? p : null;
  }

  /** Ask the agent again when its answer is old; the menu never waits. */
  function freshen(p: string) {
    if (!p || p.startsWith("__")) return;
    if (ageOf(board.find((b) => b.provider === p)) < FRESH_SECS) return;
    void refreshBoard([p]).catch(() => {});
  }

  /** Rail switch: the model pane updates to the right, cursor stays put. */
  function selectRail(p: string) {
    railSel = p;
    modelQuery = "";
    modelIndex = 0;
    freshen(p);
    setTimeout(() => searchInputEl?.focus(), 30);
  }

  function openModelPicker() {
    showModelPicker = true;
    permOpen = false;
    wsOpen = false;
    projOpen = false;
    closeInlinePops();
    modelQuery = "";
    modelIndex = 0;
    railSel =
      model === "auto"
        ? "__auto"
        : (providerOfModel(model) ??
          PROVIDER_ORDER.find((p) => board.some((r) => r.provider === p)) ??
          "");
    freshen(railSel);
    placePop(modelBtn, "model");
    api
      .getConfig()
      .then((c) => {
        favorites = c.favorite_models ?? [];
      })
      .catch(() => {});
    setTimeout(() => searchInputEl?.focus(), 30);
  }

  function togglePicker() {
    if (showModelPicker) {
      showModelPicker = false;
      return;
    }
    openModelPicker();
  }

  function toggleWs() {
    wsOpen = !wsOpen;
    if (wsOpen) {
      showModelPicker = false;
      permOpen = false;
      projOpen = false;
      closeInlinePops();
      placePop(wsBtn, "ws");
    }
  }

  function toggleProj() {
    projOpen = !projOpen;
    if (projOpen) {
      showModelPicker = false;
      permOpen = false;
      wsOpen = false;
      closeInlinePops();
      placePop(projBtn, "proj");
    }
  }

  function pickDeckProject(p: Project) {
    projOpen = false;
    dispatch("openProject", { workspace: p.workspace, slug: p.slug });
  }

  function pickWorkspace(name: string) {
    wsOpen = false;
    wsConfirm = null;
    if (name !== workspace) dispatch("workspaceChange", { workspace: name });
  }

  /** Two-step workspace delete: first click arms, second click fires. */
  let wsConfirm: string | null = null;
  let wsDeleting = false;
  $: if (!wsOpen) wsConfirm = null;
  async function deleteWorkspace(name: string) {
    if (wsDeleting) return;
    if (wsConfirm !== name) {
      wsConfirm = name;
      return;
    }
    wsConfirm = null;
    wsDeleting = true;
    try {
      await hub.deleteWorkspace(name);
      wsOpen = false;
      dispatch("workspaceDeleted", { workspace: name });
    } catch (e) {
      dispatch("error", { text: String(e) });
    } finally {
      wsDeleting = false;
    }
  }

  function togglePerm() {
    permOpen = !permOpen;
    if (permOpen) {
      showModelPicker = false;
      wsOpen = false;
      projOpen = false;
      placePop(permBtn, "perm");
    }
  }

  function onModelKey(e: KeyboardEvent) {
    if (e.ctrlKey && ["1", "2", "3", "4", "5"].includes(e.key)) {
      e.preventDefault();
      const row = mainItems[Number(e.key) - 1];
      if (row) pickRow(row);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      modelIndex = (modelIndex + 1) % Math.max(1, mainItems.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      modelIndex = (modelIndex - 1 + Math.max(1, mainItems.length)) % Math.max(1, mainItems.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const row = mainItems[modelIndex];
      if (row) pickRow(row);
    } else if (e.key === "Escape") {
      showModelPicker = false;
    }
  }

  function onWindowClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    if (showModelPicker && !el.closest(".model-zone") && !el.closest(".pop")) showModelPicker = false;
    if (permOpen && !el.closest(".perm-zone") && !el.closest(".pop")) permOpen = false;
    if (wsOpen && !el.closest(".ws-zone") && !el.closest(".pop")) wsOpen = false;
    if (projOpen && !el.closest(".proj-zone") && !el.closest(".pop")) projOpen = false;
    if (slashOpen && !el.closest(".slash-pop")) slashOpen = false;
    if (atOpen && !el.closest(".at-popup")) atOpen = false;
  }

  function shortName(name: string): string {
    const base = name.split("/").pop() ?? name;
    return base.length > 22 ? base.slice(0, 22) + "…" : base;
  }

  const IMG_EXT = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif"]);

  function isImage(name: string): boolean {
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    return IMG_EXT.has(ext);
  }

  let thumbUrls: Record<string, string | null> = {};
  let thumbReq = 0;
  $: void preloadThumbs(attachments, projectRoot);
  async function preloadThumbs(list: string[], root: string) {
    const my = ++thumbReq;
    for (const a of list) {
      if (a in thumbUrls || !isImage(a)) continue;
      try {
        const url = await api.readImageDataUrl(a, root);
        if (my !== thumbReq) return;
        thumbUrls = { ...thumbUrls, [a]: url };
      } catch {
        if (my === thumbReq) thumbUrls = { ...thumbUrls, [a]: null };
      }
    }
    if (my === thumbReq) {
      const keep = new Set(list);
      const next: Record<string, string | null> = {};
      for (const k of Object.keys(thumbUrls)) if (keep.has(k)) next[k] = thumbUrls[k];
      thumbUrls = next;
    }
  }

  function removeAttachment(a: string) {
    attachments = attachments.filter((x) => x !== a);
  }

  function clearAttachments() {
    attachments = [];
    thumbUrls = {};
  }

  let pickingFiles = false;

  function addAttachments(paths: string[]) {
    const root = (projectRoot || "").replace(/[/\\]+$/, "");
    const norm = paths
      .map((p) => {
        if (root && (p === root || p.startsWith(root + "\\") || p.startsWith(root + "/"))) {
          return p.slice(root.length).replace(/^[/\\]+/, "");
        }
        return p;
      })
      .filter((p) => p.length > 0);
    if (!norm.length) return;
    attachments = [...attachments, ...norm.filter((n) => !attachments.includes(n))].slice(0, 8);
  }

  let stageErr = "";

  async function stageImageFiles(files: FileList | File[]): Promise<void> {
    const imgs = [...files].filter(
      (f) => f.type.startsWith("image/") || isImage(f.name)
    );
    if (!imgs.length) return;
    stageErr = "";
    for (const f of imgs.slice(0, Math.max(0, 8 - attachments.length))) {
      try {
        const dataUrl = await new Promise<string>((res, rej) => {
          const r = new FileReader();
          r.onload = () => res(String(r.result ?? ""));
          r.onerror = () => rej(r.error);
          r.readAsDataURL(f);
        });
        const b64 = dataUrl.split(",", 2)[1] ?? "";
        if (!b64) continue;
        addAttachments([await api.stageImage(f.name || "pasted.png", b64)]);
      } catch (e) {
        stageErr = `Couldn't attach ${f.name || "image"}: ${e}`;
      }
    }
  }

  function onPaste(e: ClipboardEvent) {
    const files = e.clipboardData?.files;
    if (!files?.length) return;
    if (![...files].some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void stageImageFiles(files);
  }

  function onDropFiles(e: DragEvent) {
    const files = e.dataTransfer?.files;
    if (!files?.length) return;
    if (![...files].some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void stageImageFiles(files);
  }

  /** Native file explorer. Falls back to @ mention flow outside Tauri (browser dev). */
  async function pickFiles() {
    if (pickingFiles) return;
    pickingFiles = true;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const sel = await open({
        multiple: true,
        directory: false,
        defaultPath: projectRoot || undefined,
      });
      if (!sel) return;
      const list = Array.isArray(sel) ? sel : [sel];
      addAttachments(
        list.map((s) => (typeof s === "string" ? s : (s as { path: string }).path))
      );
    } catch {
      input += "@";
      textareaEl?.focus();
      handleInput();
    } finally {
      pickingFiles = false;
    }
  }
</script>

<svelte:window on:click={onWindowClick} on:keydown={handleKeydown} on:resize={repositionPops} />

<div class="ob">
  {#if workspaces.length || workspace || wsDeck.length || selDeck}
    <div class="scope-row">
      {#if workspaces.length || workspace}
        <div class="ws-zone">
          <button bind:this={wsBtn} class="scope-chip" class:open={wsOpen} on:click|stopPropagation={toggleWs}
            title={workspaceFixed ? `This chat stays in ${workspace || "Inbox"} — picking another starts a new draft` : "Workspace for this chat"}>
            <span class="chip-glyph"><Icon d={I.folder} size={12} /></span>
            <span class="truncate">{workspace || "No workspace"}</span>
            <span class="chev"><Icon d={I.chevD} size={10} /></span>
          </button>
        </div>
      {/if}
      {#if wsDeck.length || selDeck}
        <div class="proj-zone">
          <button bind:this={projBtn} class="scope-chip" class:open={projOpen} on:click|stopPropagation={toggleProj}
            title="Project in this workspace">
            <span class="chip-glyph"><Icon d={I.file} size={12} /></span>
            <span class="truncate">{projLabel}</span>
            <span class="chev"><Icon d={I.chevD} size={10} /></span>
          </button>
        </div>
      {/if}
    </div>
  {/if}
  <div
    class="ob-card"
    role="group"
    aria-label="Message composer — drop images to attach"
    on:dragover|preventDefault
    on:drop|preventDefault={onDropFiles}
    title={`${currentProject}${branch ? ` ⎇ ${branch}` : ""}${tokens > 0 ? ` · ${tokens} tok` : ""}`}
  >
    {#if attachments.length}
      <div class="attach-grid" transition:slide={{ duration: 160, easing: cubicOut }}>
        {#each attachments as a (a)}
          {@const src = thumbUrls[a] ?? null}
          <div class="thumb" class:has-img={!!src} title={a} transition:scale={{ duration: 140, start: 0.9, easing: cubicOut }}>
            {#if src}
              <img src={src} alt="" class="thumb-img" draggable="false" />
              <span class="thumb-name over">{shortName(a)}</span>
            {:else}
              <Icon d={I.file} size={16} />
              <span class="thumb-name">{shortName(a)}</span>
            {/if}
            <button class="thumb-x" title="Remove attachment" aria-label="Remove {a}" on:click={() => removeAttachment(a)}>
              <Icon d={I.close} size={10} />
            </button>
          </div>
        {/each}
        {#if attachments.length > 1}
          <button class="attach-clear" title="Remove all attachments" on:click={clearAttachments}>clear all</button>
        {/if}
      </div>
    {/if}

    <textarea
      bind:this={textareaEl}
      rows="1"
      placeholder={streaming
        ? "Working… Esc to stop"
        : mode === "web"
        ? "Search web or enter URL (e.g. github.com)... (Tab to switch)"
        : "Ask agent, / for commands, @ for context... (Tab to switch)"}
      bind:value={input}
      on:input={handleInput}
      on:paste={onPaste}
    />
    {#if stageErr}<div class="stage-err" role="alert">{stageErr}</div>{/if}

    {#if slashOpen && slashItems.length}
      <div class="slash-pop" transition:scale={{ duration: 150, start: 0.96, easing: cubicOut }}>
        {#each slashItems as cmd, i}
          <button class:on={i === slashIndex} on:click={() => runSlash(cmd)} on:mousemove={() => (slashIndex = i)}>
            <span class="n">{cmd.name}</span><span class="h">{cmd.hint}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if atOpen}
      <div class="at-popup" transition:fade={{ duration: 120 }}>
        {#each atItems.slice(0, 8) as item, i}
          <button class:on={i === atIndex} on:click={() => pickAt(item)}>{item}</button>
        {/each}
      </div>
    {/if}

    <div class="controls">
      <!-- Mode Indicator Pill with Tab Quick Toggle -->
      <div class="mode-zone ctl-zone">
        <button
          class="ctl mode-ctl"
          class:active-web={mode === "web"}
          on:click|stopPropagation={toggleMode}
          title="Switch mode: Agent or Web Browser (Press Tab)"
        >
          {#if mode === "web"}
            <span class="ctl-glyph web-icon">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1 4-10z"/></svg>
            </span>
            <span class="mode-text">Web Browser</span>
            <span class="tab-badge">Tab</span>
          {:else}
            <span class="ctl-glyph agent-icon">
              <Icon d={I.spark} size={13} />
            </span>
            <span class="mode-text">Agent Harness</span>
            <span class="tab-badge">Tab</span>
          {/if}
        </button>
      </div>

      <span class="vdiv" />

      {#if mode === "agent"}
        <div class="model-zone ctl-zone">
          <button bind:this={modelBtn} class="ctl" class:open={showModelPicker} on:click|stopPropagation={togglePicker} title="Model — open picker">
            {#if shown.provider === "auto"}
              <span class="ctl-glyph"><Icon d={I.spark} size={13} /></span>
            {:else if hasMark(shown.provider)}
              <ProviderLogo provider={shown.provider} size={13} />
            {:else}
              <span class="ctl-glyph"><Icon d={I.model} size={13} /></span>
            {/if}
            <span class="truncate">{shown.name}</span>
            <span class="chev"><Icon d={I.chevD} size={10} /></span>
          </button>
        </div>

        <span class="vdiv" />

        <div class="bars-zone" role="group" aria-label="Effort">
          {#if efforts.length}
            {#each efforts as e, i}
              <button class="bar-bit" class:lit={i <= effortIdx} on:click={() => setEffort(e)}
                title={`${effortWord(e)}${effortHint(e) ? ` — ${effortHint(e)}` : ""}`} aria-pressed={effort === e}>
                <span />
              </button>
            {/each}
          {:else}
            <span class="bars-none" title="This agent sets its own effort">–</span>
          {/if}
        </div>

        <span class="vdiv" />

        <div class="perm-zone ctl-zone">
          <button bind:this={permBtn} class="ctl" class:open={permOpen} on:click|stopPropagation={togglePerm} title="Permission policy">
            <span class="ctl-glyph"><Icon d={I.lock} size={13} /></span>
            <span class="truncate">{permTitle}</span>
            <span class="chev"><Icon d={I.chevD} size={10} /></span>
          </button>
        </div>
      {:else}
        <div class="web-zone ctl-zone">
          <span class="web-hint-pill">DuckDuckGo Search · Direct URL</span>
        </div>
      {/if}

      <span class="spacer" />

      {#if contextLimit > 0 && (contextUsed > 0 || compacting)}
        <button
          class="ctx"
          class:warn={contextPct >= 70}
          class:bad={contextPct >= 90}
          class:busy={compacting}
          disabled={compacting || streaming}
          title={compacting
            ? "Compacting the conversation…"
            : `Context ${contextPct}% full: ${kTokens(contextUsed)} of ${kTokens(contextLimit)} tokens.\nClick to compact; it also happens on its own near the limit.`}
          on:click={() => dispatch("command", { name: "compact", arg: "" })}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle class="ctx-track" cx="8" cy="8" r="6" />
            <circle
              class="ctx-fill"
              cx="8"
              cy="8"
              r="6"
              stroke-dasharray={RING}
              stroke-dashoffset={compacting ? RING * 0.7 : RING * (1 - contextPct / 100)}
            />
          </svg>
          <span>{contextPct}%</span>
        </button>
      {/if}

      <button class="icon-btn" title="Attach files" on:click={pickFiles}>
        <Icon d={I.clip} size={15} />
      </button>
      {#if streaming}
        <button class="go stop" title="Stop run" on:click={() => dispatch("stop")}>
          <Icon d={I.stop} size={11} />
        </button>
      {:else}
        <button
          class="go"
          class:ready={!!input.trim()}
          class:web-go={mode === "web"}
          title={mode === "web" ? "Browse to URL or Search (Enter)" : "Send to Agent (Enter)"}
          disabled={!input.trim()}
          on:click={submitAction}
        >
          {#if mode === "web"}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          {:else}
            <Icon d={I.sendUp} size={14} />
          {/if}
        </button>
      {/if}
    </div>
  </div>

    {#if showModelPicker}
      <div use:portal class="pop model-pop from-left" class:from-top={modelBelow} style={modelPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        <div class="pop-search">
          <Icon d={I.search} size={13} />
          <input
            bind:this={searchInputEl}
            placeholder={q ? "Search all models..." : railSel === "__fav" ? "Starred models" : railSel === "__auto" ? "Smart Auto routing" : `Search ${nameOf(railSel)} models...`}
            bind:value={modelQuery}
            on:keydown={onModelKey}
          />
          {#if $checking.has(railSel) || $checking.has("*")}
            <span class="spin" title="Asking the agent" />
          {/if}
        </div>
        <div class="pick-body">
          <div class="prov-rail">
            <button
              class="rail-row"
              class:on={railSel === "__auto"}
              on:click={() => selectRail("__auto")}
              title="Smart Auto — route across providers"
            >
              <span class="rail-auto"><Icon d={I.spark} size={15} /></span>
            </button>
            {#if favRows.length}
              <button
                class="rail-row"
                class:on={railSel === "__fav"}
                on:click={() => selectRail("__fav")}
                title="Starred models"
              >
                <span class="rail-star">★</span>
              </button>
            {/if}
            {#each PROVIDER_ORDER as p}
              {@const pr = board.find((r) => r.provider === p)}
              {#if pr}
                <button
                  class="rail-row"
                  class:on={railSel === p}
                  class:dim={!isUsable(pr)}
                  on:click={() => selectRail(p)}
                  title={isUsable(pr) ? nameOf(p) : `${nameOf(p)} — ${pr.hint}`}
                >
                  {#if hasMark(p)}
                    <ProviderLogo provider={p} size={18} />
                  {:else}
                    <span class="m-initial sm">{(p[0] ?? "?").toUpperCase()}</span>
                  {/if}
                </button>
              {/if}
            {/each}
          </div>
          <div class="model-list">
            {#if !q && railStatus && !isUsable(railStatus)}
              <div class="prov-head">
                <button
                  class="signin"
                  title="Open this agent in Settings"
                  on:click={() => { dispatch("command", { name: "keys", arg: railStatus?.provider ?? "" }); showModelPicker = false; }}
                >{railStatus.hint || "Not available"}</button>
              </div>
            {/if}
            {#each mainItems as row, i (row.value)}
              <button
                class="mrow"
                class:on={row.value === model || i === modelIndex}
                on:click={() => pickRow(row)}
                on:mousemove={() => (modelIndex = i)}
              >
                {#if q && hasMark(row.provider)}
                  <ProviderLogo provider={row.provider} size={14} />
                {/if}
                <span class="meta">
                  <span class="nm">{row.label}</span>
                  <span class="sub">{rowSub(row, board)}</span>
                </span>
                {#if i < 5}<kbd class="kbd">Ctrl+{i + 1}</kbd>{/if}
                {#if !row.usable}
                  <span class="nokey">unavailable</span>
                {/if}
                {#if row.provider !== "auto"}
                <span
                  class="star"
                  class:on={favorites.includes(row.value)}
                  role="button"
                  tabindex="0"
                  title="star model"
                  on:click|stopPropagation={() => toggleFav(row.value)}
                  on:keydown={(e) => e.key === "Enter" && toggleFav(row.value)}
                >{favorites.includes(row.value) ? "★" : "☆"}</span>
                {/if}
              </button>
            {/each}
            {#if mainItems.length === 0}
              <div class="empty">
                {$checking.has(railSel) || $checking.has("*")
                  ? "Asking the agent…"
                  : q
                    ? "No matching models found"
                    : "No models available — open Settings › Providers"}
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if wsOpen}
      <div use:portal class="pop ws-pop from-right" class:from-top={wsBelow} style={wsPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        {#if workspaceFixed}
          <div class="ws-note">This chat stays in {workspace || "Inbox"}. Picking another starts a new draft.</div>
        {/if}
        <button class="opt-row" class:on={!workspace} on:click={() => pickWorkspace("")}>
          <span class="meta"><span class="nm">No workspace</span></span>
          {#if !workspace}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
        </button>
        {#each workspaces as w (w)}
          <div class="ws-row">
            <button class="opt-row ws-pick" class:on={workspace === w} on:click={() => pickWorkspace(w)}>
              <span class="p-ico"><Icon d={I.folder} size={13} /></span>
              <span class="meta"><span class="nm">{w}</span></span>
              {#if workspace === w}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
            </button>
            <button class="ws-del" class:armed={wsConfirm === w} disabled={wsDeleting}
              title={wsConfirm === w ? `Click again to delete ${w} and its chats` : `Delete workspace ${w}`}
              on:click|stopPropagation={() => deleteWorkspace(w)}>
              {#if wsConfirm === w}sure?{:else}<Icon d={I.close} size={10} />{/if}
            </button>
          </div>
        {/each}
        <button class="opt-row new-ws" on:click={() => { wsOpen = false; dispatch("newWorkspace"); }}>
          <span class="p-ico"><Icon d={I.plus} size={13} /></span>
          <span class="meta"><span class="nm">New workspace…</span></span>
        </button>
      </div>
    {/if}

    {#if projOpen}
      <div use:portal class="pop proj-pop from-right" class:from-top={projBelow} style={projPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        {#each wsDeck as p (p.workspace + "/" + p.slug)}
          <button class="opt-row" class:on={selectedProject?.workspace === p.workspace && selectedProject?.slug === p.slug} on:click={() => pickDeckProject(p)}>
            <span class="p-ico"><Icon d={I.file} size={13} /></span>
            <span class="meta"><span class="nm">{p.title || p.slug}</span><span class="sub">{p.slug}</span></span>
            {#if selectedProject?.workspace === p.workspace && selectedProject?.slug === p.slug}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
          </button>
        {/each}
        {#if !wsDeck.length}
          <div class="empty">No projects in this workspace yet</div>
        {/if}
      </div>
    {/if}

    {#if permOpen}
      <div use:portal class="pop perm-pop from-right" class:from-top={permBelow} style={permPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        {#each PERMS as p}
          <button class="opt-row perm" class:on={permission === p.id} on:click={() => setPermission(p.id)}>
            <span class="p-ico"><Icon d={p.icon} size={14} /></span>
            <span class="meta"><span class="nm">{p.title}</span><span class="sub">{p.desc}</span></span>
          </button>
        {/each}
      </div>
    {/if}
</div>

<style>
  .ob { position: relative; width: 100%; max-width: 720px; margin: 0 auto; min-width: 0; }
  .scope-row { display: flex; align-items: center; justify-content: flex-end; gap: 4px; margin-bottom: 6px; min-width: 0; }
  .scope-chip {
    display: inline-flex; align-items: center; gap: 6px; max-width: 200px; min-width: 0;
    background: color-mix(in srgb, var(--parzi-sidebar) 62%, transparent);
    backdrop-filter: blur(10px) saturate(1.2);
    -webkit-backdrop-filter: blur(10px) saturate(1.2);
    border: 1px solid var(--line-2); border-radius: var(--radius-pill);
    color: var(--text-3); font: inherit; font-size: 11px; padding: 3px 9px; cursor: pointer;
  }
  .scope-chip:hover { color: var(--text); background: var(--surface-2); }
  .scope-chip.open { color: var(--text); border-color: var(--accent-line); }
  .scope-chip .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .scope-chip .chev { display: inline-flex; flex: none; color: var(--text-4); }
  .chip-glyph { display: inline-flex; flex: none; color: var(--text-4); }
  .ob-card {
    position: relative;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 88%, transparent), color-mix(in srgb, var(--parzi-sidebar) 78%, transparent));
    backdrop-filter: blur(14px) saturate(1.2);
    -webkit-backdrop-filter: blur(14px) saturate(1.2);
    border: 1px solid var(--line-2);
    border-top-color: var(--line-hi);
    border-radius: var(--glass-radius);
    box-shadow: var(--glass-shadow);
    padding: 14px 14px 10px;
    color: var(--text-2);
    min-width: 0;
    overflow: hidden;
    box-sizing: border-box;
  }
  .ob-card textarea {
    width: 100%; background: transparent; border: none; outline: none; resize: none;
    color: var(--text); font: inherit; font-size: 14px; line-height: 1.55;
    min-height: 26px; max-height: 180px; padding: 2px 4px 10px; box-sizing: border-box;
  }
  .ob-card textarea::placeholder { color: var(--text-4); }
  .ob-card textarea:focus { border-color: transparent !important; box-shadow: none !important; }
  .ob-card { transition: border-color 140ms ease; }
  .ob-card:focus-within { border-color: var(--line-3); }

  .attach-grid { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; overflow: hidden; }
  .thumb {
    position: relative; display: flex; align-items: center; gap: 7px;
    min-width: 0; max-width: 180px; height: 40px; padding: 0 12px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: 8px; color: var(--text-2); font-size: 12px; box-sizing: border-box;
  }
  .thumb.has-img { width: 76px; height: 58px; padding: 0; overflow: hidden; flex: none; }
  .thumb-img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .thumb-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--parzi-mono); font-size: 11px; }
  .thumb-name.over {
    position: absolute; left: 0; right: 0; bottom: 0; padding: 6px 6px 3px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.75));
    color: var(--text-2); font-size: 9px;
  }
  .thumb-x {
    position: absolute; top: -8px; right: -8px; width: 22px; height: 22px;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--menu); border: 1px solid var(--line);
    border-radius: 50%; color: var(--text-3); cursor: pointer; padding: 0;
  }
  .thumb-x:hover { color: var(--text); border-color: var(--bad); }
  .attach-clear {
    align-self: center; background: transparent; border: none; border-radius: 5px;
    color: var(--text-4); font: inherit; font-size: 11px; padding: 4px 8px; cursor: pointer;
  }
  .attach-clear:hover { color: var(--bad); background: var(--bad-soft); }
  .stage-err { font-size: 11px; color: var(--bad); padding: 2px 2px 6px; }

  .slash-pop, .at-popup {
    display: flex; flex-direction: column; gap: 1px; margin: 2px 0 8px;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 92%, transparent), color-mix(in srgb, var(--parzi-sidebar) 85%, transparent));
    backdrop-filter: blur(16px) saturate(1.25);
    -webkit-backdrop-filter: blur(16px) saturate(1.25);
    border: 1px solid var(--line-2); border-top-color: var(--line-hi);
    border-radius: 9px; padding: 5px; max-height: 220px; overflow-y: auto;
    box-shadow: var(--menu-shadow);
  }
  .slash-pop button, .at-popup button {
    display: flex; align-items: center; gap: 8px; background: transparent; border: none;
    border-radius: 6px; color: var(--text-2); text-align: left; padding: 6px 8px;
    cursor: pointer; font: inherit; font-size: 12.5px;
  }
  .slash-pop button.on, .slash-pop button:hover, .at-popup button.on, .at-popup button:hover {
    background: var(--surface-2); color: var(--text);
  }
  .slash-pop .n { flex: 1; font-family: var(--parzi-mono); font-size: 12px; }
  .slash-pop .h { margin-left: auto; font-size: 11px; color: var(--text-3); }
  .at-popup button { font-family: var(--parzi-mono); font-size: 12px; }

  .controls { display: flex; align-items: center; gap: 2px; min-width: 0; flex-wrap: wrap; row-gap: 4px; }
  .ctl-zone { position: static; min-width: 0; flex: 0 1 auto; display: flex; }
  .ctl {
    display: inline-flex; align-items: center; gap: 6px; max-width: 180px; min-width: 0; flex: 1 1 auto;
    height: 28px;
    background: transparent; border: none; border-radius: 7px; color: var(--text-3);
    font: inherit; font-size: 12px; padding: 0 7px; cursor: pointer; text-align: left;
    overflow: hidden;
  }
  .ctl:hover { background: var(--surface-2); color: var(--text); }
  .ctl.open { background: var(--surface-3); color: var(--text); }
  .ctl .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ctl .chev { color: var(--text-4); display: inline-flex; flex: none; }
  .ctl:hover .chev, .ctl.open .chev { color: var(--text-3); }
  .mode-ctl {
    padding: 0 8px;
    font-weight: 500;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .mode-ctl.active-web {
    color: #93c5fd;
    background: rgba(59, 130, 246, 0.14);
  }
  .mode-ctl.active-web:hover {
    background: rgba(59, 130, 246, 0.22);
    color: #bfdbfe;
  }
  .web-icon {
    color: #60a5fa;
    display: inline-flex;
    align-items: center;
  }
  .agent-icon {
    color: var(--accent-text);
    display: inline-flex;
    align-items: center;
  }
  .tab-badge {
    font-size: 10px;
    font-family: var(--parzi-mono, monospace);
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-3);
    padding: 1px 4px;
    border-radius: 4px;
    margin-left: 2px;
  }
  .mode-ctl.active-web .tab-badge {
    background: rgba(59, 130, 246, 0.2);
    color: #93c5fd;
  }
  .web-hint-pill {
    font-size: 11px;
    color: var(--text-4);
    display: inline-flex;
    align-items: center;
    padding: 0 6px;
    user-select: none;
  }
  .ctl-glyph { display: inline-flex; color: var(--text-4); flex: none; }
  .ctl:hover .ctl-glyph, .ctl.open .ctl-glyph { color: var(--text-2); }
  .vdiv { width: 1px; height: 16px; background: var(--line-2); margin: 0 5px; flex: none; }
  .bars-zone { display: flex; align-items: center; flex: none; padding: 0 7px; height: 28px; }
  .bars-zone .bar-bit {
    background: transparent; border: none; cursor: pointer; padding: 0 2px;
    display: flex; align-items: center; height: 28px;
  }
  .bars-zone .bar-bit span { width: 4px; height: 14px; border-radius: 2px; background: var(--surface-3); transition: background 100ms ease; }
  .bars-zone .bar-bit.lit span { background: var(--text-2); }
  .bars-zone .bar-bit:hover span { background: var(--text); }
  .bars-none { color: var(--text-4); font-size: 12px; padding: 0 6px; cursor: default; }
  .spacer { flex: 1 1 auto; min-width: 4px; }

  .ctx {
    height: 28px; flex: none; display: inline-flex; align-items: center; gap: 5px;
    padding: 0 8px; border: none; border-radius: var(--radius-pill); background: transparent;
    color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; cursor: pointer;
    --ctx: var(--text-3);
  }
  .ctx:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .ctx:disabled { cursor: default; }
  .ctx.warn { --ctx: var(--warn); color: var(--warn); }
  .ctx.bad { --ctx: var(--bad); color: var(--bad); }
  .ctx svg { transform: rotate(-90deg); }
  .ctx-track { fill: none; stroke: var(--line-3); stroke-width: 2; }
  .ctx-fill { fill: none; stroke: var(--ctx); stroke-width: 2; stroke-linecap: round; transition: stroke-dashoffset 0.4s ease; }

  .icon-btn {
    width: 32px; height: 32px; flex: none; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; border-radius: 50%; color: var(--text-3); cursor: pointer;
  }
  .icon-btn:hover { background: var(--surface-2); color: var(--text); }

  .go {
    width: 32px; height: 32px; flex: none; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-2); border: 1px solid var(--line-3);
    color: var(--text-2); cursor: pointer; padding: 0;
    box-shadow: none;
  }
  .go:hover:not(:disabled) { background: var(--surface-3); color: var(--text); }
  .go.ready:not(:disabled) { background: var(--accent); border-color: transparent; color: var(--accent-ink); }
  .go.ready:hover:not(:disabled) { background: var(--accent); color: var(--accent-ink); filter: brightness(1.08); }
  .go:active:not(:disabled) { transform: scale(0.94); }
  .go:disabled { opacity: 0.35; cursor: default; }
  .go.stop { background: var(--bad); border-color: transparent; color: var(--stage); }
  .go.web-go.ready:not(:disabled) { background: #2563eb; border-color: transparent; color: #ffffff; }
  .go.web-go.ready:hover:not(:disabled) { background: #1d4ed8; color: #ffffff; filter: brightness(1.08); }

  .pop {
    position: fixed; z-index: 60;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 92%, transparent), color-mix(in srgb, var(--parzi-sidebar) 84%, transparent));
    backdrop-filter: blur(16px) saturate(1.25);
    -webkit-backdrop-filter: blur(16px) saturate(1.25);
    border: 1px solid var(--line-2); border-top-color: var(--line-hi);
    border-radius: var(--glass-radius);
    box-shadow: var(--menu-shadow);
    overflow: hidden;
    transform-origin: bottom left;
  }
  .pop.from-right { transform-origin: bottom right; }
  .pop.from-top { transform-origin: top left; }
  .pop.from-top.from-right { transform-origin: top right; }
  .model-pop {
    width: 340px; max-width: calc(100vw - 16px);
    height: 380px; max-height: calc(100vh - 120px);
    display: flex; flex-direction: column;
  }
  .pick-body { display: flex; flex: 1; min-height: 0; }
  .prov-rail {
    flex: none; width: 46px; display: flex; flex-direction: column; gap: 2px;
    padding: 5px 4px; overflow-y: auto; border-right: 1px solid var(--line-2);
  }
  .rail-row {
    display: flex; align-items: center; justify-content: center; width: 100%; height: 32px; flex: none;
    background: transparent; border: none; border-radius: 7px; color: var(--text-2);
    padding: 0; cursor: pointer;
  }
  .rail-row:hover { background: var(--surface-2); color: var(--text); }
  .rail-row.on { background: color-mix(in srgb, var(--accent) 12%, var(--surface-2)); color: var(--text); }
  .rail-row.dim { opacity: 0.4; }
  .rail-row.dim:hover { opacity: 0.8; }
  .rail-auto { display: inline-flex; color: var(--accent-text); }
  .rail-row.on .rail-auto { color: var(--accent-text); }
  .rail-star { flex: none; color: var(--warn); font-size: 15px; line-height: 1; }
  .model-pop .model-list { border: none; padding: 5px 5px 5px 4px; max-height: none; }
  .m-initial.sm { width: 20px; height: 20px; font-size: 10px; border-radius: 6px; }
  .ws-pop, .proj-pop { width: 340px; max-width: calc(100vw - 16px); padding: 5px; overflow-y: auto; max-height: 400px; }
  .ws-pop .new-ws { color: var(--text-3); border-top: 1px solid var(--line-2); border-radius: 0 0 7px 7px; margin-top: 3px; }
  .ws-note { font-size: 11px; color: var(--text-3); line-height: 1.45; padding: 6px 8px 4px; }
  .ws-row { display: flex; align-items: center; gap: 2px; }
  .ws-row .ws-pick { width: auto; flex: 1; min-width: 0; }
  .ws-del {
    flex: none; width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; border-radius: 6px; color: var(--text-4); cursor: pointer;
    font: inherit; font-size: 11px; line-height: 1; padding: 0;
  }
  .ws-del:hover:not(:disabled) { color: var(--bad); background: var(--bad-soft); }
  .ws-del.armed { color: var(--bad); background: var(--bad-soft); width: auto; padding: 0 8px; font-weight: 600; }
  .ws-del:disabled { opacity: 0.5; cursor: default; }
  .perm-pop { width: 340px; max-width: calc(100vw - 16px); padding: 5px; overflow-y: auto; max-height: 400px; }

  .pop-search {
    display: flex; align-items: center; gap: 8px; padding: 8px 10px; flex: none;
    color: var(--text-3); border-bottom: 1px solid var(--line-2);
  }
  .pop-search input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 13px; box-shadow: none !important;
  }
  .pop-search input::placeholder { color: var(--text-4); }

  .model-list { flex: 1; min-width: 0; min-height: 0; max-height: 330px; overflow-y: auto; padding: 4px; display: flex; flex-direction: column; gap: 1px; }
  .spin {
    flex: none; width: 11px; height: 11px; border-radius: 50%;
    border: 1.6px solid var(--line-3); border-top-color: var(--text-2);
  }
  .prov-head {
    display: flex; align-items: center; gap: 8px; width: 100%;
    padding: 6px 8px 4px; color: var(--text-2);
  }

  .signin {
    margin-left: auto; flex: none; font: inherit; font-size: 11.5px; font-weight: 600;
    color: var(--accent-text); background: var(--accent-soft);
    border: 1px solid var(--accent-line); border-radius: 7px;
    padding: 4px 10px; cursor: pointer; max-width: 180px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .signin:hover { filter: brightness(1.06); }
  .mrow, .opt-row {
    display: flex; align-items: center; gap: 9px; width: 100%;
    background: transparent; border: none; border-radius: 7px; color: var(--text-2);
    text-align: left; padding: 6px 8px; cursor: pointer; font: inherit; font-size: 13px;
    transition: background 100ms ease, color 100ms ease;
  }
  .mrow:hover, .opt-row:hover { background: var(--surface-2); color: var(--text); }
  .mrow.on, .opt-row.on {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-2));
    color: var(--text);
  }
  .opt-row.on .nm { font-weight: 550; color: var(--text); }
  .opt-row.on .sub { color: var(--text-2); }
  .opt-row.on .tick { color: var(--accent); }
  .m-initial {
    width: 22px; height: 22px; flex: none; display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-2); border: 1px solid var(--line-2);
    border-radius: 6px; color: var(--text-2); font-size: 11px; font-weight: 600;
  }
  .meta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .opt-row.perm .nm { font-weight: 600; font-size: 12.5px; }
  .sub { font-size: 11px; color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; line-height: 1.4; }
  .opt-row.perm .sub { white-space: normal; }
  .kbd {
    font-family: var(--parzi-mono); font-size: 10px; color: var(--text-3); flex: none;
    border: 1px solid var(--line-3); border-radius: 4px; padding: 1px 5px;
    opacity: 0; transition: opacity 120ms ease;
  }
  .mrow:hover .kbd, .mrow.on .kbd { opacity: 1; }
  .nokey { font-size: 11px; color: var(--bad); flex: none; }
  .star { color: var(--text-4); font-size: 13px; padding: 1px 4px; cursor: pointer; flex: none; opacity: 0; transition: opacity 120ms ease; }
  .mrow:hover .star, .mrow:focus-within .star, .star.on { opacity: 1; }
  .star:hover { color: var(--text); }
  .star.on { color: var(--warn); }
  .tick { color: var(--accent); display: inline-flex; flex: none; }
  .p-ico { display: inline-flex; color: var(--text-2); flex: none; }
  .empty { padding: 14px; text-align: center; color: var(--text-3); font-size: 12px; }

  @media (prefers-reduced-motion: reduce) {
    .go:active:not(:disabled) { transform: none; }
    .kbd, .star { transition: none; }
  }
  @media (max-width: 560px) {
    .ob-card { padding: 12px 10px 8px; }
    .ctl { max-width: 128px; font-size: 12px; padding: 6px 6px; gap: 5px; }
    .vdiv { margin: 0 2px; }
  }
</style>
