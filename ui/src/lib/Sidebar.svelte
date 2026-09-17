<script lang="ts">
  import { tick } from "svelte";
  import { createEventDispatcher } from "svelte";
  import { api, deck, hub, type SessionMeta, type Project } from "./api";
  import Icon from "./Icon.svelte";
  import ThreadRow from "./ThreadRow.svelte";
  import { footerUpdateState, footerUpdateVersion } from "./updateStore";
  import { isChatThread } from "./nav";
  import { portal } from "./portal";

  export let threads: SessionMeta[] = [];
  export let activeThreadId: string | null = null;
  /** Bumped by the app after a wizard finishes, to re-read the hub rows. */
  export let hubTick = 0;

  const dispatch = createEventDispatcher<{
    selectThread: { id: string };
    newSubsession: { id: string };
    newThread: void;
    forkThread: { id: string };
    deleteThread: { id: string };
    killRun: { id: string };
    openSettings: void;
    reportIssue: void;
    openUpdates: void;
    openPalette: void;
    toggleSidebar: void;
  }>();

  const I = {
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    plus: "M12 5v14M5 12h14",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1 2 2H5a2 2 0 0 1-2-2z",
    gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12a7 7 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2z",
    issue: "M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9M13.73 21a2 2 0 0 1-3.46 0",
    update: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3",
    edit: "M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z",
    close: "M18 6L6 18M6 6l12 12",
    chevronLeft: "M15 18l-6-6 6-6",
  };

  const displayName = (n: string) => (n === "default" ? "Inbox" : n);

  let renamingId: string | null = null;
  let renameTitle = "";

  type Ctx = { kind: "thread"; id: string; title: string; x: number; y: number; confirm: boolean; count: number };
  let ctx: Ctx | null = null;

  $: byUpdated = (a: SessionMeta, b: SessionMeta) =>
    +new Date(b.updated) - +new Date(a.updated);

  $: chatThreads = threads.filter((t) => isChatThread(t.project, projectSlugs));

  // Live running chats float to the top. Coder lanes belong on the workspace.
  $: runningThreads = chatThreads.filter((t) => t.status === "active" || t.status === "queued");

  $: scopedThreads = chatThreads;

  // Running threads have their own top section, so exclude them from buckets to avoid duplicates
  $: idleScopedThreads = scopedThreads.filter((t) => t.status !== "active" && t.status !== "queued");
  $: pinned = idleScopedThreads.filter((t) => t.pinned).sort(byUpdated);

  // Hub workspaces (`~/.parzi/workspaces/*`) and the projects inside them.
  // Read here rather than passed down: the two wizards write to disk, and
  // `hubTick` is the app's way of saying "read it again".
  let hubNames: string[] = [];
  let hubProjects: Record<string, Project[]> = {};

  async function loadHub() {
    try {
      hubNames = await hub.workspaces();
    } catch {
      hubNames = [];
      return;
    }
    const pairs = await Promise.all(
      hubNames.map(async (n) => [n, await deck.list(n).catch(() => [])] as const),
    );
    hubProjects = Object.fromEntries(pairs);
  }
  $: if (hubTick >= 0) void loadHub();

  /** Role sessions (header, orchestrator, coders) carry the project slug. */
  $: projectSlugs = Object.values(hubProjects).flat().map((p) => p.slug);
  // Parent index, built once per thread list instead of once per row (E7).
  $: parentById = new Map(threads.map((x) => [x.id, x.parent_id] as const));

  // Visual indent for subsessions (full tree lives in the Agents deck).
  function depthOf(t: SessionMeta): number {
    const byId = parentById;
    let d = 0;
    let p: string | null | undefined = t.parent_id;
    let guard = 0;
    while (p && guard++ < 6) {
      d += 1;
      p = byId.get(p) ?? null;
    }
    return Math.min(d, 2);
  }

  function dayStart(d: Date): number {
    const x = new Date(d);
    x.setHours(0, 0, 0, 0);
    return +x;
  }

  interface Group {
    label: string;
    items: SessionMeta[];
  }

  // Flat T3-style list: time buckets.
  $: groups = ((): Group[] => {
    const list = idleScopedThreads.filter((t) => !t.pinned).sort(byUpdated);
    const day = 86400000;
    const t0 = dayStart(new Date());
    const b: Group[] = [
      { label: "Today", items: [] },
      { label: "Yesterday", items: [] },
      { label: "Previous 7 days", items: [] },
      { label: "Older", items: [] },
    ];
    for (const t of list) {
      const diff = Math.floor((t0 - dayStart(new Date(t.updated))) / day);
      (diff <= 0 ? b[0] : diff === 1 ? b[1] : diff <= 7 ? b[2] : b[3]).items.push(t);
    }
    return b.filter((g) => g.items.length);
  })();

  // E7: long buckets render their newest rows only; the rest is one click
  // away ("More"), so a thousand sessions cost a thousand DOM rows only if
  // you ask for them.
  const ROW_CAP = 60;
  let showAll: Record<string, boolean> = {};
  // The cap must never hide the thread you are looking at: an old selection
  // past row 60 is appended to its bucket, so the list still shows what the
  // stage shows. "Show all" stays for everything else.
  $: rowsOf = (g: Group) => {
    if (showAll[g.label]) return g.items;
    const head = g.items.slice(0, ROW_CAP);
    if (g.items.length <= ROW_CAP || !activeThreadId) return head;
    if (head.some((t) => t.id === activeThreadId)) return head;
    const active = g.items.find((t) => t.id === activeThreadId);
    return active ? [...head, active] : head;
  };

  function subtreeCount(id: string): number {
    const byParent = new Map<string, string[]>();
    for (const t of threads) {
      if (!t.parent_id) continue;
      if (!byParent.has(t.parent_id)) byParent.set(t.parent_id, []);
      byParent.get(t.parent_id)!.push(t.id);
    }
    let n = 0;
    const stack = [id];
    const seen = new Set<string>();
    while (stack.length) {
      const cur = stack.pop()!;
      if (seen.has(cur)) continue;
      seen.add(cur);
      n += 1;
      for (const k of byParent.get(cur) ?? []) stack.push(k);
    }
    return Math.max(0, n - 1);
  }

  function openThreadCtx(id: string, x: number, y: number) {
    const t = threads.find((v) => v.id === id);
    if (!t) return;
    ctx = {
      kind: "thread", id, title: t.title || "untitled",
      x: Math.min(x, window.innerWidth - 232),
      y: Math.min(y, window.innerHeight - 300),
      confirm: false, count: subtreeCount(id),
    };
  }

  async function togglePin(id: string) {
    const t = threads.find((x) => x.id === id);
    if (!t) return;
    try {
      await api.togglePin(t.id, !t.pinned);
      t.pinned = !t.pinned;
      threads = [...threads];
    } catch {}
  }

  function startRename(id: string) {
    const t = threads.find((x) => x.id === id);
    if (!t) return;
    renamingId = t.id;
    renameTitle = t.title || "untitled";
  }

  async function commitRename() {
    if (renamingId && renameTitle.trim()) {
      try {
        await api.renameThread(renamingId, renameTitle.trim());
        const target = threads.find((t) => t.id === renamingId);
        if (target) target.title = renameTitle.trim();
        threads = [...threads];
      } catch {}
    }
    renamingId = null;
  }

  async function copyText(s: string) {
    try {
      await navigator.clipboard.writeText(s);
    } catch {}
  }

  function select(id: string) {
    ctx = null;
    dispatch("selectThread", { id });
  }

  // Context-menu actions (script-side so TS narrows the ctx union properly).
  function ctxOpen() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    select(id);
  }
  function ctxSubsession() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    dispatch("newSubsession", { id });
  }
  function ctxFork() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    dispatch("forkThread", { id });
  }
  function ctxTogglePin() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    togglePin(id);
  }
  function ctxRename() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    startRename(id);
  }
  function ctxCopyId() {
    if (ctx?.kind !== "thread") return;
    copyText(ctx.id);
    ctx = null;
  }
  function ctxAskDelete() {
    if (!ctx) return;
    ctx = { ...ctx, confirm: true } as Ctx;
  }
  function ctxConfirmDeleteThread() {
    if (ctx?.kind !== "thread") return;
    const id = ctx.id;
    ctx = null;
    dispatch("deleteThread", { id });
  }
  function ctxPinnedLabel(): string {
    if (!ctx) return "Pin";
    const id = ctx.id;
    return threads.find((t) => t.id === id)?.pinned ? "Unpin" : "Pin";
  }

  // Keep the open thread visible; buckets never collapse so no unhiding needed.
  let revealedFor: string | null = null;
  $: if (activeThreadId && activeThreadId !== revealedFor) {
    revealedFor = activeThreadId;
    const rid = activeThreadId;
    tick().then(() => {
      document.getElementById(`thread-${rid}`)?.scrollIntoView({ block: "nearest" });
    });
  }
</script>

<svelte:window
  on:click={() => (ctx = null)}
  on:keydown={(e) => { if (e.key === "Escape") ctx = null; }}
/>

<aside class="sb">
  <div class="sb-head" data-tauri-drag-region>
    <button class="icon-btn" title="Hide sidebar (Ctrl+B)" on:click={() => dispatch("toggleSidebar")}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16" /></svg>
    </button>
    <img class="sb-mark" src="/mark.svg" alt="" width="22" height="22" />
    <span class="sb-wordmark">Parzi</span>
    <button class="icon-btn" title="Search (Ctrl+K)" on:click={() => dispatch("openPalette")}>
      <Icon d={I.search} size={14} />
    </button>
  </div>

  <div class="t3-top tight">
    <button class="t3-row primary" on:click={() => dispatch("newThread")} title="New chat (Ctrl+N)">
      <span class="lead"><Icon d={I.edit} size={14} /></span><span>New chat</span>
    </button>
  </div>

  <div class="sb-scroll">
    {#if runningThreads.length}
      <div class="proj-head running-head">
        <span class="pulse-dot" />
        <span>Running ({runningThreads.length})</span>
      </div>
      {#each runningThreads as t (t.id)}
        <ThreadRow
          {t} depth={0} active={t.id === activeThreadId}
          renaming={renamingId === t.id} bind:renameDraft={renameTitle}
          branch=""
          showProject={t.project && t.project !== "default" ? t.project : ""}
          on:select={(e) => select(e.detail.id)}
          on:togglePin={(e) => togglePin(e.detail.id)}
          on:startRename={(e) => { ctx = null; startRename(e.detail.id); }}
          on:commitRename={commitRename}
          on:cancelRename={() => (renamingId = null)}
          on:killRun={(e) => dispatch("killRun", { id: e.detail.id })}
          on:newSubsession={(e) => dispatch("newSubsession", { id: e.detail.id })}
          on:fork={(e) => dispatch("forkThread", { id: e.detail.id })}
          on:deleteRequest={(e) => openThreadCtx(e.detail.id, window.innerWidth - 260, 220)}
          on:contextMenu={(e) => openThreadCtx(e.detail.id, e.detail.x, e.detail.y)}
        />
      {/each}
    {/if}

    {#if pinned.length}
      <div class="proj-head"><span>★ Pinned</span></div>
      {#each pinned as t (t.id)}
        <ThreadRow
          {t} depth={0} active={t.id === activeThreadId}
          renaming={renamingId === t.id} bind:renameDraft={renameTitle}
          branch=""
          showProject={t.project && t.project !== "default" ? t.project : ""}
          on:select={(e) => select(e.detail.id)}
          on:togglePin={(e) => togglePin(e.detail.id)}
          on:startRename={(e) => { ctx = null; startRename(e.detail.id); }}
          on:commitRename={commitRename}
          on:cancelRename={() => (renamingId = null)}
          on:killRun={(e) => dispatch("killRun", { id: e.detail.id })}
          on:newSubsession={(e) => dispatch("newSubsession", { id: e.detail.id })}
          on:fork={(e) => dispatch("forkThread", { id: e.detail.id })}
          on:deleteRequest={(e) => openThreadCtx(e.detail.id, window.innerWidth - 260, 220)}
          on:contextMenu={(e) => openThreadCtx(e.detail.id, e.detail.x, e.detail.y)}
        />
      {/each}
    {/if}

    {#each groups as g (g.label)}
      <div class="proj-head"><span>{g.label}</span></div>
      {#each rowsOf(g) as t (t.id)}
        <ThreadRow
          {t} depth={depthOf(t)} active={t.id === activeThreadId}
          renaming={renamingId === t.id} bind:renameDraft={renameTitle}
          branch=""
          showProject={t.project && t.project !== "default" ? t.project : ""}
          on:select={(e) => select(e.detail.id)}
          on:togglePin={(e) => togglePin(e.detail.id)}
          on:startRename={(e) => { ctx = null; startRename(e.detail.id); }}
          on:commitRename={commitRename}
          on:cancelRename={() => (renamingId = null)}
          on:killRun={(e) => dispatch("killRun", { id: e.detail.id })}
          on:newSubsession={(e) => dispatch("newSubsession", { id: e.detail.id })}
          on:fork={(e) => dispatch("forkThread", { id: e.detail.id })}
          on:deleteRequest={(e) => openThreadCtx(e.detail.id, window.innerWidth - 260, 220)}
          on:contextMenu={(e) => openThreadCtx(e.detail.id, e.detail.x, e.detail.y)}
        />
      {/each}
      {#if !showAll[g.label] && g.items.length > ROW_CAP}
        <button class="more-row" on:click={() => (showAll = { ...showAll, [g.label]: true })}>
          More — show all {g.items.length}
        </button>
      {/if}
    {/each}

    {#if !pinned.length && !groups.length && !runningThreads.length}
      <div class="empty-state">No chats yet — start one</div>
    {/if}
  </div>

  <div class="sb-footer">
    <button class="icon-btn" title="Settings" on:click={() => dispatch("openSettings")}>
      <Icon d={I.gear} size={14} />
    </button>
    <button class="icon-btn" title="Report an issue" on:click={() => dispatch("reportIssue")}>
      <Icon d={I.issue} size={14} />
    </button>
    <button class="icon-btn upd" class:has-update={$footerUpdateState === "available"}
      title={$footerUpdateState === "available" ? `Parzi v${$footerUpdateVersion} is ready — open Updates` : $footerUpdateState === "checking" ? "Checking for updates…" : "Updates"}
      on:click={() => dispatch("openUpdates")}>
      <Icon d={I.update} size={14} />
      {#if $footerUpdateState === "available"}<span class="upd-dot" />{/if}
    </button>
  </div>
</aside>

{#if ctx}
  <!-- Real right-click menu: threads, with two-step delete. -->
  <div
    use:portal
    class="ctx-menu"
    style="left:{ctx.x}px;top:{ctx.y}px;"
    on:click|stopPropagation
    on:contextmenu|preventDefault|stopPropagation
    on:keydown={(e) => { if (e.key === "Escape") ctx = null; }}
  >
    {#if !ctx.confirm}
      <div class="ctx-title">{ctx.title}</div>
      <button class="menu-item" on:click={ctxOpen}>Open</button>
      <button class="menu-item" on:click={ctxSubsession}>New subsession</button>
      <button class="menu-item" on:click={ctxFork}>Duplicate (fork)</button>
      <button class="menu-item" on:click={ctxTogglePin}>{ctxPinnedLabel()}</button>
      <button class="menu-item" on:click={ctxRename}>Rename</button>
      <button class="menu-item" on:click={ctxCopyId}>Copy ID</button>
      <div class="menu-sep" />
      <button class="menu-item danger" on:click={ctxAskDelete}>
        Delete{ctx.count > 0 ? ` (+${ctx.count} subsession${ctx.count === 1 ? "" : "s"})` : ""}
      </button>
    {:else}
      <div class="ctx-title danger-text">Delete “{ctx.title}”?</div>
      <div class="ctx-note">{ctx.count > 0 ? `${ctx.count + 1} sessions go away, including subsessions.` : "The transcript is removed from disk."} This can't be undone.</div>
      <div class="ctx-actions">
        <button class="btn ghost" on:click={() => (ctx = null)}>Cancel</button>
        <button class="btn danger-solid" on:click={ctxConfirmDeleteThread}>Delete</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .sb {
    width: 248px; min-width: 248px; height: 100%;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 95%, transparent), color-mix(in srgb, var(--parzi-sidebar) 88%, transparent));
    border-right: 1px solid var(--line-2);
    display: flex; flex-direction: column; position: relative;
    font-size: 13px; color: var(--text-2);
  }
  .sb-head {
    display: flex; align-items: center; gap: 6px;
    padding: 10px 8px 8px;
  }
  .sb-wordmark {
    flex: 1; min-width: 0;
    font-family: "Instrument Serif", Georgia, serif; font-style: italic; font-size: 17px;
    color: var(--text); padding: 0 2px; user-select: none;
  }
  .sb-mark { width: 22px; height: 22px; border-radius: 6px; flex: none; }
  .t3-top { display: flex; flex-direction: column; gap: 2px; padding: 8px 10px 2px; }
  .t3-top.tight { padding-top: 8px; padding-bottom: 4px; }
  .lead {
    flex: none; width: 16px; height: 16px;
    display: inline-flex; align-items: center; justify-content: center;
  }
  .lead :global(svg) { display: block; }
  .t3-row {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: transparent; border: none; border-radius: 7px; color: var(--text-2);
    font: inherit; font-size: 13px; padding: 7px 8px; cursor: pointer; text-align: left;
  }
  .t3-row:hover { background: var(--surface-2); color: var(--text); }
  .t3-row.primary { color: var(--text-2); }
  .t3-row.primary:hover { background: var(--accent-soft); color: var(--text); }
  .t3-row > span:not(.lead) { flex: 1; }
  .icon-btn {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 26px; height: 26px; padding: 0 4px;
    background: transparent; border: none; border-radius: 6px;
    color: var(--text-3); cursor: pointer; font: inherit; line-height: 0;
  }
  .icon-btn:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .icon-btn.upd { position: relative; }
  .icon-btn.upd.has-update { color: var(--accent); }
  .upd-dot {
    position: absolute; top: 3px; right: 3px; width: 7px; height: 7px;
    border-radius: 50%; background: var(--accent);
    box-shadow: 0 0 6px var(--accent-glow, var(--accent));
  }
  .sb-scroll { flex: 1; overflow-y: auto; padding: 2px 10px 8px; display: flex; flex-direction: column; }


  .running-head {
    display: flex; align-items: center; gap: 6px;
    color: var(--ok); font-weight: 600;
  }
  .pulse-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--ok); box-shadow: 0 0 6px var(--ok);
    animation: pulse-ring 1.8s ease-in-out infinite; flex: none;
  }
  @keyframes pulse-ring {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.45; transform: scale(0.85); }
  }

  .proj-head {
    display: flex; align-items: center; justify-content: space-between;
    font-size: 11px; color: var(--text-3); padding: 10px 4px 4px;
  }
  .empty-state { font-size: 12px; color: var(--text-4); padding: 12px 8px; text-align: center; }
  .more-row {
    background: transparent; border: none; color: var(--text-3); font: inherit; font-size: 12px;
    padding: 7px 4px; margin: 0; cursor: pointer; text-align: left; border-radius: 7px;
  }
  .more-row:hover { color: var(--text); background: var(--surface-1); }
  .sb-footer {
    display: flex; align-items: center; gap: 2px;
    margin: 0; padding: 8px 10px 10px;
    border-top: 1px solid var(--line-2);
  }

  /* Right-click menu */
  .ctx-menu {
    position: fixed; z-index: 400; width: 220px;
    padding: 5px; display: flex; flex-direction: column; gap: 1px;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 92%, transparent), color-mix(in srgb, var(--parzi-sidebar) 85%, transparent));
    backdrop-filter: blur(16px) saturate(1.25);
    -webkit-backdrop-filter: blur(16px) saturate(1.25);
    border: 1px solid var(--line-2); border-top-color: var(--line-hi);
    border-radius: 10px; box-shadow: var(--menu-shadow);
  }
  .ctx-title {
    font-size: 12px; font-weight: 600; color: var(--text);
    padding: 6px 9px 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ctx-note { font-size: 11.5px; color: var(--text-3); padding: 2px 9px 6px; line-height: 1.45; }
  .menu-item {
    display: flex; align-items: center; gap: 9px; width: 100%;
    background: transparent; border: none; border-radius: 6px; color: var(--text-2);
    font: inherit; font-size: 12.5px; padding: 7px 9px; cursor: pointer; text-align: left;
  }
  .menu-item:hover { background: var(--surface-3); color: var(--text); }
  .menu-item.danger { color: var(--bad); }
  .menu-item.danger:hover { background: var(--bad-soft); color: var(--bad); }
  .menu-sep { height: 1px; background: var(--line-2); margin: 4px 6px; }
  .danger-text { color: var(--bad); }
  .ctx-actions { display: flex; gap: 6px; justify-content: flex-end; padding: 2px 4px 4px; }
  .btn {
    font: inherit; font-size: 12px; font-weight: 600; border-radius: 7px;
    padding: 6px 12px; cursor: pointer; border: 1px solid transparent;
  }
  .btn.ghost { background: transparent; border-color: var(--line-3); color: var(--text-2); }
  .btn.ghost:hover { background: var(--surface-2); color: var(--text); }
  .btn.danger-solid { background: var(--bad); color: #fff; }
  .btn.danger-solid:hover { filter: brightness(1.08); }
</style>
