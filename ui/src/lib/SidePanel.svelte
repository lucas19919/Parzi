<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, brain, type SessionMeta } from "./api";
  import { toast } from "./toast";
  import { mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { agentOf, isLiveSession, whereOf } from "./sessions";
  import { laneIcon } from "./lanes";
  import { formatTime as time } from "./time";

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

  interface TaskItem {
    done: boolean;
    text: string;
    depth: number;
  }
  interface TaskSection {
    title: string;
    items: TaskItem[];
  }
  interface SessionPlan {
    goal: string;
    decisions: { decision: string; why: string }[];
    steps: { title: string; status: string }[];
  }

  let goalsMd = "";
  let taskSections: TaskSection[] = [];
  let projState: "idle" | "loading" | "missing" = "idle";
  let projFor = "";
  let sessionPlan: SessionPlan | null = null;
  let planFor = "";

  function stripFront(src: string): string {
    return src.replace(/^---\s*\n[\s\S]*?\n---\s*\n/, "");
  }

  let showDecisions = false;

  function parseTasks(src: string): TaskSection[] {
    const sections: TaskSection[] = [];
    let current: TaskSection = { title: "", items: [] };
    for (const line of stripFront(src).split("\n")) {
      const head = /^#{1,3}\s+(.*)\s*$/.exec(line);
      if (head) {
        if (current.items.length || current.title) sections.push(current);
        current = { title: head[1].trim(), items: [] };
        continue;
      }
      const m = /^(\s*)[-*]\s+\[([ xX])\]\s+(.*)$/.exec(line);
      if (m) {
        current.items.push({
          done: m[2] !== " ",
          text: m[3].trim(),
          depth: Math.min(3, Math.floor(m[1].replace(/\t/g, "  ").length / 2)),
        });
      }
    }
    if (current.items.length || current.title) sections.push(current);
    return sections;
  }

  $: taskTotal = taskSections.reduce((n, s) => n + s.items.length, 0);
  $: taskDone = taskSections.reduce((n, s) => n + s.items.filter((i) => i.done).length, 0);

  async function loadProject(slug: string) {
    if (!slug) {
      projState = "idle";
      goalsMd = "";
      taskSections = [];
      projFor = "";
      return;
    }
    if (projFor === slug && projState !== "idle") return;
    projFor = slug;
    projState = "loading";
    const [goals, tasks] = await Promise.all([
      brain.read(`projects/${slug}/GOALS.md`).catch(() => ""),
      brain.read(`projects/${slug}/TASKS.md`).catch(() => ""),
    ]);
    if (projFor !== slug) return;
    goalsMd = goals;
    taskSections = tasks ? parseTasks(tasks) : [];
    projState = !goals && !tasks ? "missing" : "idle";
  }

  $: if (tab === "projects") {
    void loadProject(projectSlug);
    void loadSessionPlan(sessionId);
  }

  async function loadSessionPlan(sid: string) {
    if (!sid || planFor === sid) return;
    planFor = sid;
    sessionPlan = null;
    try {
      const raw = await api.planGet(sid);
      if (!raw.trim() || planFor !== sid) return;
      const p = JSON.parse(raw);
      sessionPlan = {
        goal: String(p.goal ?? ""),
        decisions: Array.isArray(p.decisions)
          ? p.decisions.map((d: { decision?: unknown; why?: unknown }) => ({
              decision: String(d.decision ?? ""),
              why: String(d.why ?? ""),
            }))
          : [],
        steps: Array.isArray(p.steps)
          ? p.steps.map((s: { title?: unknown; status?: unknown }) => ({
              title: String(s.title ?? ""),
              status: String(s.status ?? "todo"),
            }))
          : [],
      };
    } catch {
      if (planFor === sid) sessionPlan = null;
    }
  }

  $: planDone = sessionPlan?.steps.filter((s) => s.status === "done").length ?? 0;

  function refreshProject() {
    projFor = "";
    planFor = "";
    sessionPlan = null;
    void loadProject(projectSlug);
    void loadSessionPlan(sessionId);
  }

  async function assignTrack(section: string, text: string) {
    if (!sessionId) return;
    const where = section ? `${section} — ` : "";
    try {
      await api.spawnTrack(
        sessionId,
        text.slice(0, 60),
        `Work this track from ${projectSlug || "the project"}: ${where}${text}. Do the work, then reply with your result as the final message. Do not edit TASKS.md or GOALS.md — the orchestrator owns those files; just report back.`,
      );
      toast("Agent assigned — watch the Agents tab");
      tab = "agents";
      dispatch("settled");
    } catch (e) {
      toast(String(e), true);
    }
  }
</script>

<div class="panel" style:width="{dockW}px">
  <div class="grip" on:pointerdown={startResize} title="Drag to resize the dock" />
  <div class="p-head">
    <div class="p-tabs" role="tablist" aria-label="Dock">
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
    <button class="x" title="Close panel" on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
  </div>

  {#if tab === "tasks"}
    <div class="agents">
      {#if busyRuns.length}
        <h3>Running now</h3>
        {#each busyRuns as s (s.id)}
          {@const pill = pillOf(s)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if pill}<span class="pill {pill.cls}">{pill.label}</span>{/if}
            </button>
            <button class="stop" title="Stop this run (keeps the transcript)" on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
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
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if pill}<span class="pill {pill.cls}">{pill.label}</span>{/if}
            </button>
            <button class="stop" title="Stop this run (keeps the transcript)" on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
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
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if sub}<span class="pill {sub.cls}">{sub.label}</span>{:else}<span class="url">{whereOf(s)}</span>{/if}
            </button>
            {#if sub}
              <button class="stop" title="Stop this run (keeps the transcript)" on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
            {/if}
          </div>
        {/each}
      {:else}
        <p class="empty">No subagents yet. This session hasn't fanned anything out.</p>
      {/if}
    </div>
  {:else if tab === "projects"}
    <div class="proj">
      {#if !projectSlug && !sessionPlan}
        <p class="empty">No GOALS.md yet. Ask the agent to draft the goal and break it into tasks.</p>
      {:else}
        {#if sessionPlan}
          <section class="card" aria-label="Session plan">
            <div class="eyebrow">This session</div>
            <div class="card-title">{sessionPlan.goal || "Session plan"}</div>
            {#if sessionPlan.steps.length}
              <div class="proj-bar"><i style:width="{Math.round((planDone / Math.max(1, sessionPlan.steps.length)) * 100)}%" /></div>
              <div class="prog">{planDone}/{sessionPlan.steps.length} steps done</div>
            {/if}
            {#each sessionPlan.steps as st, i (i)}
              <div class="trow">
                <span class="box" class:doing={st.status === "doing"} class:done={st.status === "done"}>{#if st.status === "done"}x{/if}</span>
                <span class="ttext" class:done={st.status === "done"}>{st.title}</span>
                {#if st.status === "doing"}<span class="pill live">doing</span>{/if}
              </div>
            {/each}
            {#if sessionPlan.decisions.length}
              <button class="disclosure" aria-expanded={showDecisions} on:click={() => (showDecisions = !showDecisions)}>
                <span class="tri" class:open={showDecisions}>▸</span>
                <span>{sessionPlan.decisions.length} decision{sessionPlan.decisions.length === 1 ? "" : "s"}</span>
              </button>
              {#if showDecisions}
                {#each sessionPlan.decisions as d, i (i)}
                  <div class="dec"><b>{d.decision}</b>{#if d.why}<span> — {d.why}</span>{/if}</div>
                {/each}
              {/if}
            {/if}
          </section>
        {/if}
        {#if projectSlug}
          <section class="card" aria-label="Project plan">
            <div class="eyebrow">Project · {projectSlug}</div>
            {#if taskTotal}
              <div class="proj-bar"><i style:width="{Math.round((taskDone / Math.max(1, taskTotal)) * 100)}%" /></div>
              <div class="prog">{taskDone}/{taskTotal} tracks done</div>
            {/if}
            {#if goalsMd}
              <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
              <div class="goals" on:click={(e) => void handleLinkClick(e)}>{@html mdHtml(stripFront(goalsMd), $richReady)}</div>
            {/if}
            {#each taskSections as sec, si (si)}
              {#if sec.title}<h4>{sec.title}</h4>{/if}
              {#each sec.items as item, ii (ii)}
                <div class="trow" style:padding-left="{8 + item.depth * 14}px">
                  <span class="box" class:done={item.done}>{#if item.done}x{/if}</span>
                  <span class="ttext" class:done={item.done}>{item.text}</span>
                  {#if !item.done}
                    <button class="assign" title="Assign an agent to this track" on:click={() => assignTrack(sec.title, item.text)}>Assign</button>
                  {/if}
                </div>
              {/each}
            {/each}
            <button class="mini" title="Reload GOALS.md and TASKS.md" on:click={refreshProject}>Reload files</button>
          </section>
        {/if}
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
    color: var(--faint);
    font-size: 12px;
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
    font-size: 10px;
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
    color: var(--faint);
    cursor: pointer;
  }
  .x:hover { background: var(--line); color: var(--text); }
  h3 {
    margin: 12px 12px 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .empty { margin: 24px 16px; font-size: 12.5px; color: var(--faint); line-height: 1.6; }
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
  .row { display: flex; align-items: center; gap: 2px; padding: 2px 8px 2px 4px; }
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
  .time { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .icon { flex: none; display: inline-flex; color: var(--muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
  .url { flex: none; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; color: var(--faint); }
  .pill { flex: none; padding: 1px 8px; border-radius: 999px; font-size: 10.5px; font-weight: 600; }
  .pill.live { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .pill.queue { background: color-mix(in srgb, var(--warn) 16%, transparent); color: var(--warn); }
  .stop {
    flex: none;
    padding: 4px 10px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 11.5px;
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
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
    margin-bottom: 6px;
  }
  .card-title { font-weight: 650; font-size: 13.5px; color: var(--text); margin-bottom: 8px; }
  .prog { font-size: 11px; color: var(--faint); margin: 4px 2px 2px; font-variant-numeric: tabular-nums; }
  .disclosure {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding: 2px 0;
    background: none;
    border: none;
    color: var(--faint);
    font-size: 11.5px;
    cursor: pointer;
  }
  .disclosure:hover { color: var(--text); }
  .tri { display: inline-block; font-size: 10px; transition: transform 140ms ease; }
  .tri.open { transform: rotate(90deg); }
  .mini {
    padding: 3px 9px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 11px;
    cursor: pointer;
  }
  .mini:hover { color: var(--text); }
  .proj-bar {
    height: 5px;
    margin: 0 12px 4px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .proj-bar i { display: block; height: 100%; background: var(--ok); border-radius: inherit; }
  .proj h4 {
    margin: 12px 12px 2px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .goals { margin: 0 12px; font-size: 12.5px; line-height: 1.6; color: var(--muted); }
  .trow { display: flex; align-items: baseline; gap: 8px; padding: 3px 12px 3px 0; }
  .trow .box {
    flex: none;
    width: 14px;
    height: 14px;
    border: 1px solid var(--faint);
    border-radius: 4px;
    font-size: 10px;
    line-height: 13px;
    text-align: center;
    color: var(--on-ok);
    transform: translateY(2px);
  }
  .trow .box.done { background: var(--ok); border-color: var(--ok); }
  .trow .box.doing { border-color: var(--warn); }
  .dec { margin: 2px 12px; font-size: 12.5px; line-height: 1.5; color: var(--muted); }
  .dec b { color: var(--text); font-weight: 600; }
  .ttext { font-size: 12.5px; color: var(--text); flex: 1; min-width: 0; }
  .ttext.done { color: var(--faint); text-decoration: line-through; }
  .assign {
    flex: none;
    padding: 2px 8px;
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--faint);
    font-size: 10.5px;
    cursor: pointer;
    opacity: 0;
  }
  .trow:hover .assign { opacity: 1; }
  .assign:hover { color: var(--text); border-color: var(--faint); }
</style>
