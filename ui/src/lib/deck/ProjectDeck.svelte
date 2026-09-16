<script lang="ts">
  /**
   * The project deck (PLAN.md §8): Project / Plan / Activity over one
   * project, with the Ask box that talks to the header and — on Direct —
   * to the orchestrator. Round 1 is one machine: the poll below is the
   * stand-in for the hub's push channel.
   */
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import DeckHeader from "./DeckHeader.svelte";
  import ProjectTab from "./ProjectTab.svelte";
  import PlanTab from "./PlanTab.svelte";
  import ActivityTab from "./ActivityTab.svelte";
  import { api, deck, onRunEvent, type AuditResult, type ChatEvent, type Draft, type InspectorDoc, type JournalLine, type Plan, type Project, type ProjectOpen, type UiEvent } from "../api";
  import { deckFixture, deckFixtureStateAsync, type DeckTab } from "./fixtures";
  import { deckDrafts, etagOf, liveLaneCount, resolveWorkspace, sprintPosition, taskStates, type DeckApproval } from "./state";

  export let workspace = "";
  export let slug = "";
  /** Screenshot override; normally comes from the host's `ui_state`. */
  export let uiState: string | null = null;

  const dispatch = createEventDispatcher<{
    /** A draft asked to be read in the right deck's Docs tab. */
    openDoc: { doc: InspectorDoc };
    error: { text: string };
  }>();

  const POLL_MS = 2000;

  let tab: DeckTab = "project";
  let ws = workspace;
  let open: ProjectOpen | null = null;
  let project: Project | null = null;
  let plan: Plan = { sprints: [] };
  let journal: JournalLine[] = [];
  let drafts: Draft[] = [];
  /** STATUS.md text — rendered, not a model answer (PLAN.md §15.7). */
  let status = "";
  let audit: AuditResult | null = null;
  let auditing = "";
  let approving = false;
  let error = "";
  /** True when the fixtures drive the deck: no commands, no poll. */
  let fixture = false;

  // Etags: the poll assigns only what changed, so cards do not re-render.
  let tags = { project: "", plan: "", journal: "", drafts: "", status: "" };

  // The role thread under the Ask box.
  let role: "header" | "orchestrator" = "header";
  let session = "";
  let events: ChatEvent[] = [];
  let liveText = "";
  let liveTools: { id: string; name: string; label: string; running: boolean; ok: boolean; ms: number }[] = [];
  let streaming = false;

  /** Task id → the `critical` transfer card waiting for a person. */
  let approvals: Record<string, DeckApproval> = {};

  let timer: ReturnType<typeof setInterval> | null = null;
  let unlisten: (() => void) | null = null;
  let loadedKey = "";

  $: live = taskStates(plan, journal);
  $: sprint = plan.sprints.length ? sprintPosition(plan, live) : { n: 0, m: 0 };
  $: lanesLive = liveLaneCount(live);
  $: people = [...new Set(journal.filter((l) => !l.who.startsWith("lane ")).map((l) => l.who))].slice(0, 4);
  $: waiting = Object.keys(approvals).length;
  $: deckDrafts.set(drafts.map((d) => ({ label: d.name.replace(/\.md$/, ""), path: d.path })));

  // Reload whenever the deck is pointed at another project. The first run
  // fires with an empty slug too, so a `deck:*` fixture (the shell mounts
  // the deck with no project for screenshots) still gets to draw.
  $: if (`${workspace}/${slug}` !== loadedKey) {
    loadedKey = `${workspace}/${slug}`;
    void boot();
  }

  async function boot() {
    error = "";
    const state = await deckFixtureStateAsync(uiState);
    if (state) {
      const f = deckFixture(state);
      fixture = true;
      tab = f.tab;
      project = f.project;
      ws = f.project.workspace;
      plan = f.plan;
      journal = f.journal;
      drafts = f.drafts;
      status = f.status;
      audit = f.audit;
      return;
    }
    fixture = false;
    if (!slug) {
      project = null;
      error = "No project open.";
      return;
    }
    try {
      ws = workspace || (await resolveWorkspace(slug));
      open = await deck.open(ws, slug);
      project = open.project;
      session = open.header_session;
      await loadThread();
      await refresh();
    } catch (e) {
      fail(e);
    }
  }

  /** One poll round; each command is independent, a missing one is not fatal. */
  async function refresh() {
    if (fixture || !slug || !ws) return;
    const [p, pl, jr, dr, st] = await Promise.allSettled([
      deck.get(ws, slug),
      deck.plan(ws, slug),
      deck.journal(ws, slug),
      deck.drafts(ws, slug),
      deck.status(ws, slug),
    ]);
    if (p.status === "fulfilled") assign("project", p.value, (v) => (project = v));
    if (pl.status === "fulfilled") assign("plan", pl.value, (v) => (plan = v));
    if (jr.status === "fulfilled") assign("journal", jr.value, (v) => (journal = v));
    if (dr.status === "fulfilled") assign("drafts", dr.value, (v) => (drafts = v));
    if (st.status === "fulfilled") assign("status", st.value, (v) => (status = v.text));
  }

  function assign<T>(key: keyof typeof tags, value: T, set: (v: T) => void) {
    const tag = etagOf(value);
    if (tags[key] === tag) return;
    tags[key] = tag;
    set(value);
  }

  async function loadThread() {
    if (!session) {
      events = [];
      return;
    }
    try {
      const [, ev] = await api.getThread(session);
      events = ev;
    } catch {
      events = [];
    }
    liveText = "";
    liveTools = [];
  }

  async function ask(e: CustomEvent<{ prompt: string; to: "header" | "orchestrator" }>) {
    if (!open || fixture) return;
    if (e.detail.to !== role) {
      role = e.detail.to;
      session = (role === "header" ? open.header_session : open.orchestrator_session) ?? "";
      await loadThread();
    }
    events = [...events, { kind: "user", text: e.detail.prompt }];
    streaming = true;
    liveText = "";
    liveTools = [];
    try {
      const id = await deck.ask(open, e.detail.to, e.detail.prompt);
      session = id;
      if (role === "orchestrator" && open) open = { ...open, orchestrator_session: id };
    } catch (err) {
      streaming = false;
      fail(err);
    }
  }

  async function runAudit(e: CustomEvent<{ draft: Draft }>) {
    if (fixture || !project) return;
    auditing = e.detail.draft.name;
    try {
      audit = await deck.audit(ws, slug, e.detail.draft.name);
      await refresh();
    } catch (err) {
      fail(err);
    } finally {
      auditing = "";
    }
  }

  async function approvePlan() {
    if (fixture || !project) return;
    approving = true;
    try {
      project = await deck.approve(ws, slug);
      audit = null;
      tab = "plan";
      await refresh();
    } catch (err) {
      fail(err);
    } finally {
      approving = false;
    }
  }

  /** The transfer card answers through the same gate a tool approval does. */
  async function answerApproval(e: CustomEvent<{ key: string; allow: boolean }>) {
    const { [approvalTask(e.detail.key)]: _gone, ...rest } = approvals;
    approvals = rest;
    if (fixture) return;
    try {
      await api.approveTool(e.detail.key, e.detail.allow);
    } catch (err) {
      fail(err);
    }
  }

  function approvalTask(key: string): string {
    return Object.keys(approvals).find((t) => approvals[t].key === key) ?? "";
  }

  function openDraft(e: CustomEvent<{ draft: Draft }>) {
    const d = e.detail.draft;
    dispatch("openDoc", { doc: { title: d.title || d.name, content: d.content, path: d.path } });
  }

  function onEvent(ev: UiEvent) {
    if (ev.kind === "approval") {
      const args = (ev.call.args ?? {}) as Record<string, unknown>;
      const task = String(args.task ?? args.for_task ?? args.for ?? "");
      if (!task) return; // Not a project transfer: the thread view owns it.
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
    if (!("session" in ev) || ev.session !== session) return;
    if (ev.kind === "text") liveText += ev.text;
    else if (ev.kind === "tool_call") {
      if (!liveTools.some((t) => t.id === ev.id)) {
        liveTools = [...liveTools, { id: ev.id, name: ev.name, label: ev.label || ev.name, running: true, ok: true, ms: 0 }];
      }
    } else if (ev.kind === "tool_result") {
      let joined = false;
      liveTools = liveTools.map((t) => {
        if (!joined && t.running && (t.id === ev.id || t.name === ev.name)) {
          joined = true;
          return { ...t, running: false, ok: ev.ok, ms: ev.ms };
        }
        return t;
      });
    } else if (ev.kind === "done" || ev.kind === "error") {
      streaming = false;
      if (ev.kind === "error") fail(ev.error);
      void loadThread();
      void refresh();
    }
  }

  function fail(e: unknown) {
    error = String(e);
    dispatch("error", { text: error });
  }

  onMount(async () => {
    unlisten = await onRunEvent(onEvent);
    timer = setInterval(() => {
      if (typeof document !== "undefined" && document.hidden) return;
      void refresh();
    }, POLL_MS);
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
    if (unlisten) unlisten();
    deckDrafts.set([]);
  });
</script>

<div class="deck">
  {#if project}
    <DeckHeader
      title={project.title}
      workspace={ws}
      {sprint}
      {lanesLive}
      {people}
      {tab}
      {waiting}
      on:tab={(e) => (tab = e.detail.tab)}
    />

    {#if error}
      <div class="err" role="status">{error}</div>
    {/if}

    <div class="body">
      {#if tab === "project"}
        <ProjectTab
          {project}
          {drafts}
          {audit}
          {auditing}
          {approving}
          {events}
          {liveText}
          {liveTools}
          {streaming}
          canDirect={!fixture}
          on:ask={ask}
          on:audit={runAudit}
          on:approve={approvePlan}
          on:openDraft={openDraft}
        />
      {:else if tab === "plan"}
        <PlanTab {plan} {live} {approvals} status={project.status} on:approve={answerApproval} />
      {:else}
        <ActivityTab {journal} {status} />
      {/if}
    </div>
  {:else}
    <div class="loading">{error || "Opening project…"}</div>
  {/if}
</div>

<style>
  .deck {
    max-width: 1180px; margin: 0 auto; width: 100%; box-sizing: border-box;
    padding: 12px 28px 96px; display: flex; flex-direction: column; gap: 14px;
  }
  .body { min-width: 0; }
  .err {
    background: var(--bad-soft); border: 1px solid var(--bad-line); border-radius: var(--radius-2);
    color: var(--text); font-size: 12px; padding: 7px 11px;
  }
  .loading { color: var(--text-3); font-size: 13px; padding: 48px 4px; }
</style>
