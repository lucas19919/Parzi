<script lang="ts">
  /**
   * The project stage: its conversation and the Ask box, nothing else.
   * Lanes and settings live in the right panel (ProjectPanel) and read the
   * store this publishes. Round 1 is one machine: the poll stands in for push.
   */
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import Thread from "../Thread.svelte";
  import Omnibar from "../Omnibar.svelte";
  import { ensureModels, modelRows } from "../modelStore";
  import { api, deck, onRunEvent, type AuditResult, type ChatEvent, type Draft, type InspectorDoc, type JournalLine, type Plan, type Project, type ProjectOpen, type UiEvent } from "../api";
  import { deckFixture, deckFixtureStateAsync } from "./fixtures";
  import { deckDrafts, etagOf, liveLaneCount, projectPanel, resolveWorkspace, taskStates, type DeckApproval } from "./state";

  export let workspace = "";
  export let slug = "";
  /** Screenshot override; normally comes from the host's `ui_state`. */
  export let uiState: string | null = null;

  const dispatch = createEventDispatcher<{
    /** A draft asked to be read in the right deck's Docs tab. */
    openDoc: { doc: InspectorDoc };
    error: { text: string };
    /** Show the Project tab of the right panel. */
    openPanel: void;
  }>();

  const POLL_MS = 2000;

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

  // The role thread on the stage. The message box's mode picks the role:
  // Chat talks to the header, Plan and Build to the orchestrator.
  let role: "header" | "orchestrator" = "header";
  let input = "";
  let attachments: string[] = [];
  let effort: "low" | "medium" | "high" | "extra" | "ultra" = "medium";
  let mode: "chat" | "plan" | "build" = "chat";
  /** Empty = the roster model of whichever role the mode picks. */
  let pickedModel = "";
  $: to = (mode === "chat" ? "header" : "orchestrator") as "header" | "orchestrator";
  $: shownModel = pickedModel || (project ? (to === "header" ? project.roster.header : project.roster.orchestrator) : "");
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
  $: lanesLive = liveLaneCount(live);
  $: waiting = Object.keys(approvals).length;
  $: projectPanel.set(
    project
      ? {
          project, plan, live, approvals, drafts, audit, auditing, approving,
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

  // Switching the mode shows that role's thread before anything is sent.
  $: if (open && to !== role) void switchRole(to);
  async function switchRole(next: "header" | "orchestrator") {
    if (!open) return;
    role = next;
    session = (role === "header" ? open.header_session : open.orchestrator_session) ?? "";
    await loadThread();
  }

  async function ask() {
    const prompt = input.trim();
    if (!open || fixture || !prompt || streaming) return;
    const files = [...attachments];
    input = "";
    attachments = [];
    events = [...events, { kind: "user", text: prompt }];
    streaming = true;
    liveText = "";
    liveTools = [];
    try {
      const id = await deck.ask(open, role, prompt, { model: pickedModel, effort, attachments: files });
      session = id;
      if (role === "orchestrator" && open) open = { ...open, orchestrator_session: id };
    } catch (err) {
      streaming = false;
      input = prompt;
      attachments = files;
      fail(err);
    }
  }

  async function stop() {
    if (!session) return;
    try {
      await api.killRun(session);
    } catch (err) {
      fail(err);
    }
  }

  async function runAudit(d: Draft) {
    if (fixture || !project) return;
    auditing = d.name;
    try {
      audit = await deck.audit(ws, slug, d.name);
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
      await refresh();
    } catch (err) {
      fail(err);
    } finally {
      approving = false;
    }
  }

  /** The transfer card answers through the same gate a tool approval does. */
  async function answerApproval(key: string, allow: boolean) {
    const { [approvalTask(key)]: _gone, ...rest } = approvals;
    approvals = rest;
    if (fixture) return;
    try {
      await api.approveTool(key, allow);
    } catch (err) {
      fail(err);
    }
  }

  function approvalTask(key: string): string {
    return Object.keys(approvals).find((t) => approvals[t].key === key) ?? "";
  }

  function openDraft(d: Draft) {
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

  async function saveProject(next: Project) {
    if (fixture) {
      project = next;
      return;
    }
    try {
      project = await deck.save(next);
    } catch (err) {
      fail(err);
    }
  }

  onMount(async () => {
    void ensureModels(false);
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
    projectPanel.set(null);
  });
</script>

<div class="deck">
  {#if project}
    <header class="head">
      <span class="title" title={project.title}>{project.title}</span>
      <span class="status">{project.status}</span>
      <span class="spacer" />
      <button class="lanes-btn" class:live={lanesLive > 0} class:waiting={waiting > 0} title="Lanes and settings" on:click={() => dispatch("openPanel")}>
        <span class="dot" />
        {#if waiting}{waiting} waiting{:else if lanesLive}{lanesLive} running{:else}Lanes{/if}
      </button>
    </header>

    {#if error}
      <div class="err" role="status">{error}</div>
    {/if}

    <div class="body">
      {#if events.length || streaming}
        <Thread {events} {liveText} {liveTools} {streaming} />
      {:else}
        <div class="empty">
          <span class="e-title">Nothing said yet</span>
          <span class="e-sub">Talk the project through here. Lanes and settings sit behind the button up top.</span>
        </div>
      {/if}
    </div>
    <div class="ask-dock">
      <Omnibar
        bind:input
        bind:attachments
        bind:effort
        bind:mode
        model={shownModel}
        models={$modelRows}
        streaming={streaming}
        currentProject={project.slug}
        on:modelChange={(e) => (pickedModel = e.detail.model)}
        on:send={ask}
        on:stop={stop}
      />
    </div>
  {:else}
    <div class="loading">{error || "Opening project…"}</div>
  {/if}
</div>

<style>
  .deck {
    max-width: 820px; margin: 0 auto; width: 100%; height: 100%; box-sizing: border-box;
    padding: 12px 28px 0; display: flex; flex-direction: column; gap: 10px; min-height: 0;
  }
  .head { display: flex; align-items: baseline; gap: 10px; flex: none; padding: 4px 2px 0; min-width: 0; }
  .title { font-size: 18px; font-weight: 650; color: var(--text); letter-spacing: -0.2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { font-size: 12px; color: var(--text-4); flex: none; }
  .spacer { flex: 1; }
  .lanes-btn {
    align-self: center; flex: none; display: inline-flex; align-items: center; gap: 7px; height: 26px; padding: 0 11px;
    background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-pill);
    color: var(--text-3); font: inherit; font-size: 12px; cursor: pointer;
  }
  .lanes-btn:hover { color: var(--text); background: var(--surface-2); }
  .lanes-btn .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--text-4); }
  .lanes-btn.live { color: var(--text-2); }
  .lanes-btn.live .dot { background: var(--ok); }
  .lanes-btn.waiting { color: var(--text); border-color: var(--warn-line); }
  .lanes-btn.waiting .dot { background: var(--warn); }
  .body { min-width: 0; flex: 1; overflow: auto; }
  .body :global(.thread-col) { padding: 8px 0 24px; max-width: none; }
  .empty { display: flex; flex-direction: column; gap: 6px; padding: 72px 2px 0; }
  .e-title { font-size: 15px; color: var(--text-2); font-weight: 600; }
  .e-sub { font-size: 12.5px; color: var(--text-4); }
  .ask-dock { flex: none; padding: 8px 0 18px; }
  .err {
    background: var(--bad-soft); border: 1px solid var(--bad-line); border-radius: var(--radius-2);
    color: var(--text); font-size: 12px; padding: 7px 11px;
  }
  .loading { color: var(--text-3); font-size: 13px; padding: 48px 4px; }
</style>
