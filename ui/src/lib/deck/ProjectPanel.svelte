<script lang="ts">
  /**
   * The Project tab of the right panel. No project open: the workspace's
   * projects. A project open: its lanes and settings. The conversation owns
   * the stage; data and actions for an open project come from the deck's store.
   */
  import { createEventDispatcher, onDestroy } from "svelte";
  import SettingsTab from "./SettingsTab.svelte";
  import {
    deckDrafts, etagOf, laneNames, projectPanel, restingState, taskStates,
    type DeckApproval, type TaskLive,
  } from "./state";
  import {
    api, deck, onRunEvent,
    type AuditResult, type Draft, type InspectorDoc, type JournalLine,
    type Plan, type Project, type ProjectOpen, type Task, type UiEvent,
  } from "../api";

  export let workspace = "";
  export let projects: Project[] = [];
  /** Project selected in the dock: the panel boots its home itself, so the
      old full-stage deck is gone — project work lives here exclusively. */
  export let selected: { workspace: string; slug: string } | null = null;

  const dispatch = createEventDispatcher<{
    openProject: { workspace: string; slug: string };
    newProject: { workspace: string };
    closeProject: void;
    renameProject: { workspace: string; slug: string; title: string };
    deleteProject: { workspace: string; slug: string };
    openSession: { id: string };
    openDraft: { doc: InspectorDoc };
    error: { text: string };
  }>();

  const POLL_MS = 2000;

  let open: ProjectOpen | null = null;
  let plan: Plan = { sprints: [] };
  let journal: JournalLine[] = [];
  let drafts: Draft[] = [];
  let audit: AuditResult | null = null;
  let auditing = "";
  let approving = false;
  /** The `critical` transfer cards waiting for a person (same gate as tool
      approvals; anything else the global handler owns). */
  let approvals: Record<string, DeckApproval> = {};
  let bootError = "";
  let timer: ReturnType<typeof setInterval> | null = null;
  let unlisten: (() => void) | null = null;
  let loadedKey = "";
  let tags = { project: "", plan: "", journal: "", drafts: "" };

  $: live = taskStates(plan, journal);
  $: if (selected) void bootHome(selected);
  else stopHome();

  // The home publishes exactly what it used to when the stage deck owned it:
  // lanes/settings read the store, drafts feed the Docs tab's quick tabs.
  $: projectPanel.set(
    selected && open
      ? {
          project: open.project, plan, live, approvals, drafts, audit, auditing, approving,
          actions: {
            audit: (d) => void runAudit(d),
            approve: () => void approvePlan(),
            answer: (key, allow) => void answerApproval(key, allow),
            save: (p) => void saveProject(p),
            openDraft,
          },
        }
      : null,
  );
  $: deckDrafts.set(drafts.map((d) => ({ label: d.name.replace(/\.md$/, ""), path: d.path })));

  async function bootHome(sel: { workspace: string; slug: string }) {
    const key = `${sel.workspace}/${sel.slug}`;
    if (key === loadedKey) return;
    stopHome();
    loadedKey = key;
    section = "lanes";
    try {
      open = await deck.open(sel.workspace, sel.slug);
      await refresh();
      unlisten = await onRunEvent(onHomeEvent);
      timer = setInterval(() => {
        if (typeof document !== "undefined" && document.hidden) return;
        void refresh();
      }, POLL_MS);
    } catch (e) {
      bootError = String(e);
      dispatch("error", { text: bootError });
    }
  }

  function stopHome() {
    if (timer) clearInterval(timer);
    timer = null;
    if (unlisten) unlisten();
    unlisten = null;
    loadedKey = "";
    open = null;
    plan = { sprints: [] };
    journal = [];
    drafts = [];
    audit = null;
    auditing = "";
    approving = false;
    approvals = {};
    bootError = "";
    projectPanel.set(null);
    deckDrafts.set([]);
  }

  onDestroy(stopHome);

  /** One poll round; each command is independent, a missing one is not fatal. */
  async function refresh() {
    if (!selected || !open) return;
    const { workspace: ws, slug } = selected;
    const [p, pl, jr, dr] = await Promise.allSettled([
      deck.get(ws, slug),
      deck.plan(ws, slug),
      deck.journal(ws, slug),
      deck.drafts(ws, slug),
    ]);
    if (p.status === "fulfilled") assign("project", p.value, (v) => (open = open && { ...open, project: v }));
    if (pl.status === "fulfilled") assign("plan", pl.value, (v) => (plan = v));
    if (jr.status === "fulfilled") assign("journal", jr.value, (v) => (journal = v));
    if (dr.status === "fulfilled") assign("drafts", dr.value, (v) => (drafts = v));
  }

  function assign<T>(key: keyof typeof tags, value: T, set: (v: T) => void) {
    const tag = etagOf(value);
    if (tags[key] === tag) return;
    tags[key] = tag;
    set(value);
  }

  async function runAudit(d: Draft) {
    if (!selected || !open) return;
    auditing = d.name;
    try {
      audit = await deck.audit(selected.workspace, selected.slug, d.name);
      await refresh();
    } catch (err) {
      dispatch("error", { text: String(err) });
    } finally {
      auditing = "";
    }
  }

  async function approvePlan() {
    if (!selected) return;
    approving = true;
    try {
      audit = null;
      await deck.approve(selected.workspace, selected.slug);
      await refresh();
    } catch (err) {
      dispatch("error", { text: String(err) });
    } finally {
      approving = false;
    }
  }

  /** Transfer cards answer through the same gate a tool approval does. */
  async function answerApproval(key: string, allow: boolean) {
    const { [approvalTask(key)]: _gone, ...rest } = approvals;
    approvals = rest;
    try {
      await api.approveTool(key, allow);
    } catch (err) {
      dispatch("error", { text: String(err) });
    }
  }

  function approvalTask(key: string): string {
    return Object.keys(approvals).find((t) => approvals[t].key === key) ?? "";
  }

  function onHomeEvent(ev: UiEvent) {
    if (ev.kind === "approval") {
      const args = (ev.call.args ?? {}) as Record<string, unknown>;
      const task = String(args.task ?? args.for_task ?? args.for ?? "");
      if (!task) return; // Not a project transfer: the global handler owns it.
      approvals = {
        ...approvals,
        [task]: {
          key: ev.key,
          task,
          lane: ev.call.lane,
          name: ev.call.name,
          detail: JSON.stringify(args, null, 2).slice(0, 1200),
        },
      };
      return;
    }
    if (!("session" in ev) || !open) return;
    if (ev.session !== open.header_session && ev.session !== open.orchestrator_session) return;
    if (ev.kind === "done" || ev.kind === "error") void refresh();
  }

  async function saveProject(next: Project) {
    try {
      await deck.save(next);
      await refresh();
    } catch (err) {
      dispatch("error", { text: String(err) });
    }
  }

  function openDraft(d: Draft) {
    dispatch("openDraft", { doc: { title: d.title || d.name, content: d.content, path: d.path } });
  }

  function openSession(id: string) {
    if (id) dispatch("openSession", { id });
  }

  /** Inline rename + two-step delete for the workspace's project list. */
  let renamingSlug: string | null = null;
  let renameDraft = "";
  let delConfirm: string | null = null;

  function focusRename(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function commitRename(proj: Project) {
    const title = renameDraft.trim();
    renamingSlug = null;
    if (title && title !== (proj.title || proj.slug)) {
      dispatch("renameProject", { workspace, slug: proj.slug, title });
    }
  }

  type LaneState = "running" | "blocked" | "done" | "idle";
  interface LaneRow {
    name: string;
    state: LaneState;
    done: number;
    total: number;
    /** The task the lane is on, or the next one it would take. */
    current: Task | null;
    holder: string;
    detail: string;
    approvals: DeckApproval[];
  }

  let section: "lanes" | "settings" = "lanes";

  $: p = $projectPanel;
  $: rows = p ? laneRows(p.plan, p.live, p.approvals) : [];
  $: totalDone = rows.reduce((a, r) => a + r.done, 0);
  $: total = rows.reduce((a, r) => a + r.total, 0);
  $: canStart = !!p && (p.project.status === "drafting" || p.project.status === "planned") && (!!p.audit || total > 0);

  function laneRows(plan: Plan, live: Record<string, TaskLive>, approvals: Record<string, DeckApproval>): LaneRow[] {
    return laneNames(plan).map((name) => {
      const tasks = plan.sprints.flatMap((s) => s.lanes.filter((l) => l.name === name).flatMap((l) => l.tasks));
      const st = (t: Task) => live[t.id] ?? restingState(t);
      const done = tasks.filter((t) => st(t).state === "done").length;
      const active = tasks.find((t) => ["running", "claimed", "blocked"].includes(st(t).state));
      const current = active ?? tasks.find((t) => st(t).state === "pending") ?? null;
      const cur = current ? st(current) : null;
      const state: LaneState =
        tasks.some((t) => st(t).state === "blocked") ? "blocked"
        : active ? "running"
        : tasks.length && done === tasks.length ? "done"
        : "idle";
      const detail = cur?.state === "running" && cur.tool ? `${cur.tool} · turn ${cur.turn}` : "";
      return {
        name,
        state,
        done,
        total: tasks.length,
        current: state === "done" ? null : current,
        // Lane agents sign the journal as "lane <name>": the row already says that.
        holder: cur?.holder && cur.holder !== `lane ${name}` ? cur.holder : "",
        detail,
        approvals: Object.values(approvals).filter((a) => a.lane === name || tasks.some((t) => t.id === a.task)),
      };
    });
  }

  const LABEL: Record<LaneState, string> = { running: "running", blocked: "blocked", done: "done", idle: "free" };
</script>

{#if selected && p}
  <div class="panel">
    <button class="back" on:click={() => dispatch("closeProject")}>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6" /></svg>
      {p.project.workspace || workspace || "Projects"}
    </button>
    <div class="head">
      <span class="title" title={p.project.title}>{p.project.title}</span>
      <span class="status">{p.project.status}</span>
    </div>
    <div class="discuss">
      <button class="btn" disabled={!open} on:click={() => openSession(open?.header_session ?? "")}
        title="Open the header's session as a chat on the stage">Discuss with header</button>
      <button class="btn" disabled={!open?.orchestrator_session} on:click={() => openSession(open?.orchestrator_session ?? "")}
        title={open?.orchestrator_session ? "Open the orchestrator's session as a chat on the stage" : "The orchestrator starts with the first audit"}>Discuss with orchestrator</button>
    </div>

    <div class="seg" role="tablist">
      <button class:on={section === "lanes"} role="tab" aria-selected={section === "lanes"} on:click={() => (section = "lanes")}>
        Lanes{#if rows.length}<span class="n">{rows.length}</span>{/if}
      </button>
      <button class:on={section === "settings"} role="tab" aria-selected={section === "settings"} on:click={() => (section = "settings")}>
        Settings
      </button>
    </div>

    <div class="body">
      {#if section === "lanes"}
        {#if p.audit}
          <div class="ready">
            <span class="r-title">Plan ready</span>
            <pre class="r-body">{p.audit.summary}</pre>
          </div>
        {/if}

        {#if canStart}
          <button class="start" disabled={p.approving} on:click={() => p?.actions.approve()}>
            {p.approving ? "Starting…" : "Start lanes"}
          </button>
        {/if}

        {#if rows.length}
          <div class="progress" title="{totalDone} of {total} tasks done">
            <span class="bar"><i style="width: {total ? (totalDone / total) * 100 : 0}%" /></span>
            <span class="pct">{totalDone}/{total}</span>
          </div>
          <ul class="lanes">
            {#each rows as r (r.name)}
              <li class="lane {r.state}">
                <div class="l-top">
                  <span class="dot" />
                  <span class="l-name">{r.name}</span>
                  <span class="spacer" />
                  <span class="l-count">{r.done}/{r.total}</span>
                  <span class="l-state">{LABEL[r.state]}</span>
                </div>
                {#if r.current}
                  <div class="l-task" title={r.current.title}>{r.current.title}</div>
                {/if}
                {#if r.holder || r.detail}
                  <div class="l-sub">{[r.holder, r.detail].filter(Boolean).join(" · ")}</div>
                {/if}
                {#each r.approvals as a (a.key)}
                  <div class="ask">
                    <span>Allow <b>{a.name}</b>?</span>
                    <span class="spacer" />
                    <button class="btn primary" on:click={() => p?.actions.answer(a.key, true)}>Allow</button>
                    <button class="btn" on:click={() => p?.actions.answer(a.key, false)}>Deny</button>
                  </div>
                {/each}
              </li>
            {/each}
          </ul>
        {:else if !p.audit}
          <p class="empty">No lanes yet. Talk the project through, then switch the message box to Plan to draft them.</p>
          {#if p.drafts.length}
            <ul class="drafts">
              {#each p.drafts as d (d.path)}
                <li>
                  <button class="d-name" title="Read" on:click={() => p?.actions.openDraft(d)}>{d.title || d.name}</button>
                  <button class="btn" disabled={!!p.auditing} on:click={() => p?.actions.audit(d)}>
                    {p.auditing === d.name ? "Planning…" : "Make lanes"}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      {:else}
        <SettingsTab project={p.project} on:save={(e) => p?.actions.save(e.detail.project)} />
      {/if}
    </div>
  </div>
{:else if selected}
  <div class="panel">
    <div class="body"><p class="empty">{bootError || "Opening project…"}</p></div>
  </div>
{:else if workspace}
  <div class="panel">
    <div class="head">
      <span class="title">{workspace}</span>
      <span class="status">projects</span>
    </div>
    <div class="body">
      {#each projects as proj (proj.slug)}
        <div class="proj-row">
          {#if renamingSlug === proj.slug}
            <input class="rename" bind:value={renameDraft} use:focusRename
              on:keydown={(e) => {
                if (e.key === "Enter") commitRename(proj);
                else if (e.key === "Escape") renamingSlug = null;
              }} />
            <button class="mini-btn" title="Save" on:click={() => commitRename(proj)}>✓</button>
            <button class="mini-btn" title="Cancel" on:click={() => (renamingSlug = null)}>✕</button>
          {:else}
            <button class="proj" on:click={() => dispatch("openProject", { workspace, slug: proj.slug })}>
              <span class="p-title">{proj.title || proj.slug}</span>
              <span class="p-status">{String(proj.status).toLowerCase()}</span>
            </button>
            <button class="mini-btn" title="Rename {proj.title || proj.slug}"
              on:click={() => { renamingSlug = proj.slug; renameDraft = proj.title || proj.slug; delConfirm = null; }}>✎</button>
            <button class="mini-btn danger" class:armed={delConfirm === proj.slug}
              title={delConfirm === proj.slug ? "Click again to delete it and its role sessions" : `Delete ${proj.title || proj.slug}`}
              on:click={() => {
                if (delConfirm !== proj.slug) delConfirm = proj.slug;
                else { delConfirm = null; dispatch("deleteProject", { workspace, slug: proj.slug }); }
              }}>{delConfirm === proj.slug ? "sure?" : "✕"}</button>
          {/if}
        </div>
      {:else}
        <p class="empty">No projects in {workspace} yet.</p>
      {/each}
      <button class="new" on:click={() => dispatch("newProject", { workspace })}>+ New project</button>
    </div>
  </div>
{/if}

<style>
  .panel { display: flex; flex-direction: column; min-height: 0; height: 100%; }
  .back {
    align-self: flex-start; display: inline-flex; align-items: center; gap: 4px; margin: 10px 10px 0;
    background: none; border: none; padding: 3px 6px; border-radius: var(--radius-1);
    color: var(--text-3); font: inherit; font-size: 11.5px; cursor: pointer;
  }
  .back:hover { color: var(--text); background: var(--surface-1); }
  .proj-row { display: flex; align-items: center; gap: 4px; }
  .proj-row .proj { flex: 1; min-width: 0; }
  .proj-row .rename { flex: 1; min-width: 0; }
  .mini-btn {
    flex: none; min-width: 26px; height: 26px; padding: 0 4px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; border-radius: 6px;
    color: var(--text-4); font: inherit; font-size: 12px; cursor: pointer;
  }
  .mini-btn:hover { color: var(--text); background: var(--surface-2); }
  .mini-btn.danger:hover, .mini-btn.danger.armed { color: var(--bad); background: var(--bad-soft); }
  .mini-btn.armed { font-size: 10px; font-weight: 600; }
  .proj {
    display: flex; align-items: center; gap: 8px; width: 100%; text-align: left;
    padding: 9px 10px; border-radius: var(--radius-2); background: var(--surface-1);
    border: 1px solid var(--line-2); color: var(--text); font: inherit; cursor: pointer;
  }
  .proj:hover { background: var(--surface-2); }
  .p-title { flex: 1; min-width: 0; font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .p-status { font-size: 11px; color: var(--text-4); flex: none; }
  .new {
    align-self: flex-start; background: none; border: none; padding: 4px 2px;
    color: var(--text-3); font: inherit; font-size: 12.5px; cursor: pointer;
  }
  .new:hover { color: var(--text); }
  .head { display: flex; align-items: baseline; gap: 8px; padding: 14px 14px 10px; min-width: 0; }
  .discuss { display: flex; gap: 6px; padding: 0 14px 10px; }
  .title { font-size: 14px; font-weight: 650; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { font-size: 11px; color: var(--text-4); flex: none; }

  .seg { display: flex; gap: 2px; padding: 0 12px 10px; border-bottom: 1px solid var(--line-2); }
  .seg button {
    display: inline-flex; align-items: center; gap: 6px; height: 26px; padding: 0 10px;
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-2);
    color: var(--text-3); font: inherit; font-size: 12px; font-weight: 500; cursor: pointer;
  }
  .seg button:hover { color: var(--text); background: var(--surface-1); }
  .seg button.on { color: var(--text); background: var(--surface-2); border-color: var(--line-2); }
  .n { font-size: 10px; color: var(--text-4); }

  .body { flex: 1; min-height: 0; overflow: auto; padding: 12px 14px 24px; display: flex; flex-direction: column; gap: 10px; }
  .body :global(.set) { max-width: none; padding: 0 0 24px; }

  .progress { display: flex; align-items: center; gap: 8px; }
  .bar { flex: 1; height: 3px; border-radius: 3px; background: var(--surface-3); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); }
  .pct { font-size: 11px; color: var(--text-4); font-variant-numeric: tabular-nums; }

  .lanes, .drafts { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .lane {
    padding: 9px 10px; border-radius: var(--radius-2); background: var(--surface-1);
    border: 1px solid var(--line-2); display: flex; flex-direction: column; gap: 3px; min-width: 0;
  }
  .l-top { display: flex; align-items: center; gap: 7px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--text-4); flex: none; }
  .lane.running .dot { background: var(--ok); box-shadow: 0 0 0 3px var(--ok-soft); }
  .lane.blocked .dot { background: var(--warn); }
  .lane.done .dot { background: var(--text-4); opacity: 0.5; }
  .l-name { font-size: 13px; font-weight: 600; color: var(--text); }
  .spacer { flex: 1; }
  .l-count { font-size: 11px; color: var(--text-4); font-variant-numeric: tabular-nums; }
  .l-state { font-size: 11px; color: var(--text-3); min-width: 48px; text-align: right; }
  .lane.running .l-state { color: var(--ok); }
  .lane.blocked .l-state { color: var(--warn); }
  .l-task { font-size: 12px; color: var(--text-2); padding-left: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lane.done .l-name { color: var(--text-3); }
  .l-sub { font-size: 11px; color: var(--text-4); padding-left: 13px; font-family: var(--parzi-mono); }

  .ask {
    margin-top: 5px; display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-2);
    padding: 6px 8px; border-radius: var(--radius-1); background: var(--warn-soft); border: 1px solid var(--warn-line);
  }
  .btn {
    background: var(--surface-2); border: 1px solid var(--line-2); color: var(--text);
    font: inherit; font-size: 11.5px; padding: 3px 10px; border-radius: var(--radius-1); cursor: pointer;
  }
  .btn:hover:not(:disabled) { background: var(--surface-3); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn.primary { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent-text); }

  .start {
    align-self: stretch; background: var(--accent); border: none; color: var(--accent-ink);
    font: inherit; font-size: 12.5px; font-weight: 600; padding: 8px 12px; border-radius: var(--radius-2); cursor: pointer;
  }
  .start:disabled { opacity: 0.6; cursor: default; }
  .ready { display: flex; flex-direction: column; gap: 6px; }
  .r-title { font-size: 12px; font-weight: 650; color: var(--text); }
  .r-body {
    margin: 0; font-family: inherit; font-size: 12px; line-height: 1.55; color: var(--text-2);
    white-space: pre-wrap; max-height: 220px; overflow: auto;
  }
  .empty { margin: 4px 0; font-size: 12.5px; line-height: 1.5; color: var(--text-4); }
  .drafts li { display: flex; align-items: center; gap: 8px; }
  .d-name {
    flex: 1; min-width: 0; text-align: left; background: none; border: none; padding: 0; font: inherit;
    font-size: 12.5px; color: var(--text-2); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .d-name:hover { color: var(--text); }
</style>
