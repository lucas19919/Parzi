<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, brain, taskApi, type SessionMeta, type Task } from "./api";
  import { toastError } from "./toast";
  import { mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { agentOf, isLiveSession, whereOf } from "./sessions";
  import { laneIcon } from "./lanes";
  import { formatTime as time } from "./time";
  import { tooltip } from "./tooltip";
  import { openTask, refreshTasks, tasks, tasksError } from "./taskStore";
  import { byOrder, inScope, statusLabel, waitsOn } from "./taskModel";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let sessionId = "";
  export let projectSlug = "";
  // Dock tabs are a registry: append { id, label } plus a content branch
  // below to add projects, checklists, etc. without touching the shell.
  type DockTab = "agents" | "projects" | "tasks";
  const DOCK_TABS: { id: DockTab; label: string }[] = [
    { id: "agents", label: "Agents" },
    { id: "projects", label: "Projects" },
    { id: "tasks", label: "Tasks" },
  ];

  export let tab: DockTab = "agents";

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    stopSession: { id: string };
    close: void;
    settled: void;
  }>();

  interface McpConn {
    name: string;
    on: boolean;
  }

  let poller = 0;
  export let dockW = 360;

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = dockW;
    const move = (ev: PointerEvent) => {
      dockW = Math.min(640, Math.max(280, Math.round(startW + (startX - ev.clientX))));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      try {
        localStorage.setItem("parzi.dock.w", String(dockW));
      } catch {}
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function laneFallback(s: SessionMeta): "brain" | "bot" | "chat" {
    return laneIcon(s.lane ?? "");
  }

  function isLive(s: SessionMeta) {
    return isLiveSession(s, running);
  }

  function pillOf(s: SessionMeta): { label: string; cls: string } | null {
    if (isLive(s)) return { label: "Running", cls: "live" };
    if (s.status === "queued") return { label: "Queued", cls: "queue" };
    return null;
  }

  $: family = (() => {
    if (!sessionId) return [];
    const byId = new Map(threads.map((t) => [t.id, t]));
    const out: SessionMeta[] = [];
    for (const t of threads) {
      let cur = t.parent_id;
      let guard = 0;
      while (cur && guard++ < 50) {
        if (cur === sessionId) {
          out.push(t);
          break;
        }
        cur = byId.get(cur)?.parent_id;
      }
    }
    return out;
  })();
  $: pool = family.map((t) => ({ ...t, at: Date.parse(t.updated) || 0 }));
  $: activeList = pool.filter((t) => isLive(t) || t.status === "queued").sort((a, b) => b.at - a.at);
  $: rows = (() => {
    const kids = new Map<string, typeof pool>();
    for (const t of pool) {
      const p = t.parent_id ?? "";
      if (!kids.has(p)) kids.set(p, []);
      kids.get(p)?.push(t);
    }
    const byId = new Map(pool.map((t) => [t.id, t]));
    const roots = (kids.get(sessionId) ?? []).concat(pool.filter((t) => t.parent_id && t.parent_id !== sessionId && !byId.has(t.parent_id)));
    const byTime = (a: { at: number }, b: { at: number }) => b.at - a.at;
    roots.sort(byTime);
    for (const arr of kids.values()) arr.sort(byTime);
    const out: { s: (typeof pool)[number]; depth: number }[] = [];
    const walk = (id: string, depth: number) => {
      for (const c of kids.get(id) ?? []) {
        out.push({ s: c, depth });
        walk(c.id, depth + 1);
      }
    };
    for (const r of roots) {
      out.push({ s: r, depth: 0 });
      walk(r.id, 1);
    }
    return out;
  })();

  $: busyRuns = threads
    .filter((t) => isLive(t) || t.status === "queued")
    .map((t) => ({ ...t, at: Date.parse(t.updated) || 0 }))
    .sort((a, b) => b.at - a.at);

  let conns: McpConn[] = [];
  let connsFor = "";
  let connsErr = "";

  async function loadConns() {
    if (connsFor) return;
    connsFor = "loading";
    try {
      const cfg = await api.getConfig();
      const servers = (cfg.mcp?.servers ?? {}) as Record<string, { enabled?: boolean }>;
      conns = Object.entries(servers).map(([name, s]) => ({ name, on: s.enabled !== false }));
      connsErr = "";
      connsFor = "done";
    } catch (e) {
      // Shown in place of the list; reopening the tab tries again.
      connsErr = String(e);
      connsFor = "";
    }
  }

  function poke() {
    clearInterval(poller);
    if (tab === "tasks") {
      void loadConns();
      dispatch("settled");
      poller = window.setInterval(() => dispatch("settled"), 3000);
    }
  }

  $: tab, poke();

  onDestroy(() => clearInterval(poller));

  let goalsMd = "";
  let projState: "idle" | "loading" | "missing" = "idle";
  let projFor = "";

  function stripFront(src: string): string {
    return src.replace(/^---\s*\n[\s\S]*?\n---\s*\n/, "");
  }

  async function loadProject(slug: string) {
    if (!slug) {
      projState = "idle";
      goalsMd = "";
      projFor = "";
      return;
    }
    if (projFor === slug && projState !== "idle") return;
    projFor = slug;
    projState = "loading";
    const goals = await brain.read(`projects/${slug}/GOALS.md`).catch(() => "");
    if (projFor !== slug) return;
    goalsMd = goals;
    projState = goals ? "idle" : "missing";
  }

  $: if (tab === "projects") void loadProject(projectSlug);

  function refreshProject() {
    projFor = "";
    void loadProject(projectSlug);
  }

  // This session's project tasks, live: in progress first, then what came
  // back for review, then what is left to do (ready before blocked).
  const RANK: Record<string, number> = { doing: 0, review: 1, failed: 1, todo: 2 };
  $: projectTasks = $tasks
    .filter((t) => t.status !== "done" && inScope(t, projectSlug))
    .sort((a, b) => RANK[a.status] - RANK[b.status] || Number(b.ready) - Number(a.ready) || byOrder(a, b));
  $: doneCount = $tasks.filter((t) => t.status === "done" && inScope(t, projectSlug)).length;
  let taskDraft = "";

  async function addTask() {
    const title = taskDraft.trim();
    if (!title) return;
    try {
      await taskApi.create(title, projectSlug || null);
      taskDraft = "";
      void refreshTasks();
    } catch (e) {
      toastError(e);
    }
  }

  async function markDone(t: Task) {
    try {
      await taskApi.update(t.id, { status: "done" });
      void refreshTasks();
    } catch (e) {
      toastError(e);
    }
  }

</script>

<div class="panel" style:width="{dockW}px">
  <div class="grip" on:pointerdown={startResize} title="Drag to resize the dock" />
  <div class="p-head">
    <div class="p-tabs" role="tablist" aria-label="Dock" data-ui-segmented>
      {#each DOCK_TABS as t (t.id)}
        <button
          role="tab"
          aria-selected={tab === t.id}
          class:on={tab === t.id}
          on:click={() => (tab = t.id)}
        >
          {t.label}{#if t.id === "agents" && activeList.length}<span class="n">{activeList.length}</span>{/if}
        </button>
      {/each}
    </div>
    <button class="x" use:tooltip={"Close panel"} on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
  </div>

  {#if tab === "tasks"}
    <div class="agents">
      <div class="t-head">
        <h3>{projectSlug ? `Tasks in ${projectSlug}` : "Inbox tasks"}</h3>
        <button class="mini" on:click={() => openTask()}>Open tasks</button>
      </div>
      <form class="t-new" on:submit|preventDefault={addTask}>
        <Icon name="plus" size={12} />
        <input aria-label="Add a task" placeholder="Add a task" bind:value={taskDraft} />
      </form>
      {#each projectTasks as t (t.id)}
        <div class="row">
          <button class="check" role="checkbox" aria-checked="false" use:tooltip={"Mark done"} on:click={() => markDone(t)} />
          <button class="open" title={t.blocked_by.length ? waitsOn(t, $tasks) : t.title} on:click={() => openTask(t.id)}>
            <span class="title">{t.title}</span>
            {#if t.status !== "todo"}
              <span class="pill {t.status === 'doing' ? 'live' : t.status === 'failed' ? 'bad' : 'queue'}">{statusLabel(t.status)}</span>
            {:else if t.blocked_by.length}
              <span class="url">Blocked</span>
            {/if}
          </button>
        </div>
      {:else}
        <p class="empty">{$tasksError ? `Couldn't load tasks: ${$tasksError}` : doneCount ? "Everything here is done." : "No tasks yet. Add one, or ask the agent to plan with tasks."}</p>
      {/each}
      {#if busyRuns.length}
        <h3>Running now</h3>
        {#each busyRuns as s (s.id)}
          {@const pill = pillOf(s)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if pill}<span class="pill {pill.cls}">{pill.label}</span>{/if}
            </button>
            <button class="stop" use:tooltip={"Stop this run (keeps the transcript)"} on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
          </div>
        {/each}
      {:else}
        <p class="empty">Nothing running. Spawned agents and queued runs land here.</p>
      {/if}
      <h3>Connections</h3>
      {#each conns as c (c.name)}
        <div class="row">
          <span class="dot" class:on={c.on} />
          <span class="title">{c.name}</span>
          <span class="url">{c.on ? "connected" : "off"}</span>
        </div>
      {:else}
        {#if connsErr}
          <p class="empty err">Couldn't load connectors: {connsErr}</p>
        {:else}
          <p class="empty">No connectors yet. Add one in Settings › Connections.</p>
        {/if}
      {/each}
    </div>
  {:else if tab === "agents"}
    <div class="agents">
      {#if activeList.length}
        <h3>Active now</h3>
        {#each activeList as s (s.id)}
          {@const pill = pillOf(s)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if pill}<span class="pill {pill.cls}">{pill.label}</span>{/if}
            </button>
            <button class="stop" use:tooltip={"Stop this run (keeps the transcript)"} on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
          </div>
        {/each}
      {/if}
      {#if rows.length}
        <h3>This session</h3>
        {#each rows as { s, depth } (s.id)}
          {@const sub = pillOf(s)}
          <div class="row" style:padding-left="{depth * 16}px">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if sub}<span class="pill {sub.cls}">{sub.label}</span>{:else}<span class="url">{whereOf(s)}</span>{/if}
            </button>
            {#if sub}
              <button class="stop" use:tooltip={"Stop this run (keeps the transcript)"} on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
            {/if}
          </div>
        {/each}
      {:else}
        <p class="empty">No subagents yet. This session hasn't fanned anything out.</p>
      {/if}
    </div>
  {:else if tab === "projects"}
    <div class="proj">
      {#if !projectSlug}
        <p class="empty">This session has no project. Pick a folder in the composer to give it one.</p>
      {:else}
        <section class="card" aria-label="Project goals">
          <div class="eyebrow">Project · {projectSlug}</div>
          {#if goalsMd}
            <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
            <div class="goals" on:click={(e) => void handleLinkClick(e)}>{@html mdHtml(stripFront(goalsMd), $richReady)}</div>
          {:else if projState === "missing"}
            <p class="goals">No GOALS.md in this project's notes yet. Tasks live in the Tasks tab.</p>
          {/if}
          <button class="mini" use:tooltip={"Reload GOALS.md"} on:click={refreshProject}>Reload</button>
        </section>
      {/if}
    </div>
  {/if}
</div>

<style>
  .panel {
    position: relative;
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
    background: transparent;
  }
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    width: 9px;
    cursor: ew-resize;
    z-index: 5;
  }
  .grip:hover::after,
  .grip:active::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
    opacity: 0.7;
  }
  .p-head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 10px 6px;
  }
  .p-tabs {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--text) 5%, transparent);
  }
  .p-tabs button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    background: none;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .p-tabs button.on {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    color: var(--text);
  }
  .n {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--ok);
    color: var(--on-ok);
    font-size: var(--fs-xs);
    font-weight: 700;
    line-height: 16px;
  }
  .x {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover { background: var(--line); color: var(--text); }
  h3 {
    margin: 12px 12px 4px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .empty { margin: 24px 16px; font-size: var(--fs-md); color: var(--muted); line-height: 1.6; }
  .empty.err { color: var(--bad); }
  .agents { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: 16px; }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    margin-left: 8px;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.on { background: var(--ok); }
  .row { display: flex; align-items: center; gap: 8px; padding: 2px 8px 2px 4px; }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  .open:hover { background: var(--line); color: var(--text); }
  .time { flex: none; font-size: var(--fs-xs); color: var(--muted); font-variant-numeric: tabular-nums; }
  .icon { flex: none; display: inline-flex; color: var(--muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-md); }
  .url { flex: none; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-xs); color: var(--muted); }
  .pill { flex: none; padding: 1px 8px; border-radius: 999px; font-size: var(--fs-xs); font-weight: 600; }
  .pill.live { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .pill.queue { background: color-mix(in srgb, var(--warn) 16%, transparent); color: var(--warn); }
  .stop {
    flex: none;
    padding: 4px 10px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .stop:hover { border-color: var(--bad); color: var(--bad); }
  .proj { flex: 1; min-height: 0; overflow-y: auto; padding: 10px 10px 16px; display: flex; flex-direction: column; gap: 10px; }
  .card {
    background: color-mix(in srgb, var(--panel) 60%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    padding: 10px 12px 12px;
  }
  .eyebrow {
    font-size: var(--fs-xs);
    font-weight: 700;
    color: var(--muted);
    margin-bottom: 6px;
  }
  .mini {
    min-height: 24px;
    padding: 3px 9px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-xs);
    cursor: pointer;
  }
  .mini:hover { color: var(--text); }
  .goals { margin: 0 12px; font-size: var(--fs-md); line-height: 1.6; color: var(--muted); }
  .goals + .mini { margin-top: 10px; }
  .t-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding-right: 12px; }
  .t-new {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 12px 6px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .t-new input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .check {
    position: relative;
    flex: none;
    width: 24px;
    height: 24px;
    margin-left: 4px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .check::before {
    content: "";
    position: absolute;
    inset: 5px;
    border: 1px solid var(--muted);
    border-radius: var(--radius-sm);
  }
  .check:hover { background: var(--hover); }
  .pill.bad { background: color-mix(in srgb, var(--bad) 24%, transparent); color: var(--text); }
</style>
