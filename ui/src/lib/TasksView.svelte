<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import { brain, taskApi, type Project, type SessionMeta, type Task, type TaskPatch, type TaskStatus } from "./api";
  import { board as providers } from "./providerStore";
  import { persisted } from "./browserData";
  import { isLiveSession } from "./sessions";
  import { permissionBlocked, permissionFor } from "./lanes";
  import { tooltip } from "./tooltip";
  import { toast, toastError } from "./toast";
  import { assignPrefs, refreshTasks, targetTask, tasks, tasksError, tasksLoaded } from "./taskStore";
  import {
    ALL,
    COLUMNS,
    STATUSES,
    board,
    columnOf,
    dropped,
    moved,
    needCandidates,
    nextStatus,
    projectFor,
    projectLabel,
    sameProject,
    scopeOptions,
    statusLabel,
    upsert,
    waitsOn,
    type Column,
  } from "./taskModel";
  import { api } from "./api";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  // The same `mode` strings the composer sends (see Omnibar's PERMS).
  const PERMS = [
    { id: "deny", label: "Read only" },
    { id: "supervised", label: "Ask first" },
    { id: "edits", label: "Auto-accept edits" },
    { id: "auto", label: "Auto" },
    { id: "full", label: "Full access" },
  ];

  const scope = persisted<string>("parzi.tasks.scope.v1", ALL);
  let projects: Project[] = [];
  let policy = "";
  let draft = "";
  let selected = "";
  let editing = "";
  let editText = "";
  let notesDraft = "";
  let notesFor = "";
  let notesSeen = "";
  let notesFocused = false;
  let projectsLoaded = false;
  let assigning = false;
  let dragId = "";
  let dropOn: { col: Column; before: string | null } | null = null;
  let lane = "";

  $: titles = new Map(projects.map((p) => [p.slug.toLowerCase(), p.title]));
  $: options = scopeOptions($tasks, projects);
  // A remembered project that is gone falls back to All, once both lists are in.
  $: if (projectsLoaded && $tasksLoaded && !options.some((o) => o.value === $scope)) scope.set(ALL);
  $: cols = board($tasks, $scope);
  $: sel = $tasks.find((t) => t.id === selected) ?? null;
  $: if (sel && notesFor !== sel.id) {
    notesFor = sel.id;
    notesDraft = sel.notes;
    notesSeen = sel.notes;
    lane = sel.lane || "build";
  }
  // An agent's note arrives while the panel is open: show it unless typing.
  $: if (sel && notesFor === sel.id && sel.notes !== notesSeen) {
    notesSeen = sel.notes;
    if (!notesFocused) notesDraft = sel.notes;
  }
  $: candidates = sel ? needCandidates(sel, $tasks).filter((t) => !sel?.needs.includes(t.id)) : [];
  $: mode = $assignPrefs.mode || permissionFor(policy);
  $: blockedMode = permissionBlocked(mode, policy);
  $: scopeName = options.find((o) => o.value === $scope)?.label ?? "All projects";

  onMount(() => {
    void refreshTasks();
    brain
      .projects()
      .then((p) => (projects = p))
      .catch(() => {})
      .finally(() => (projectsLoaded = true));
    api
      .getConfig()
      .then((cfg) => (policy = cfg.lanes?.default_mode ?? ""))
      .catch(() => {});
    const onPolicy = (e: Event) => (policy = (e as CustomEvent<string>).detail ?? "");
    document.addEventListener("parzi:default-mode", onPolicy);
    return () => document.removeEventListener("parzi:default-mode", onPolicy);
  });

  // Opened at a task (a toast, Home, the side panel): show and select it.
  $: if ($targetTask && $tasksLoaded) void reveal($targetTask);

  async function reveal(id: string) {
    targetTask.set("");
    const t = $tasks.find((x) => x.id === id);
    if (!t) return;
    if ($scope !== ALL && !sameProject(t.project, $scope || null)) scope.set(ALL);
    selected = id;
    await tick();
    document.querySelector(`[data-task="${id}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function sessionOf(t: Task): SessionMeta | null {
    return t.session ? (threads.find((s) => s.id === t.session) ?? null) : null;
  }

  function agentState(t: Task): { label: string; cls: string } | null {
    const s = sessionOf(t);
    if (!s) return null;
    if (isLiveSession(s, running)) return { label: "Running", cls: "live" };
    if (s.status === "queued") return { label: "Queued", cls: "queue" };
    return null;
  }

  function sessionLabel(s: SessionMeta): string {
    if (isLiveSession(s, running)) return "Running";
    if (s.status === "queued") return "Waiting for a free slot";
    if (s.status === "killed") return "Stopped";
    if (s.status === "done") return "Finished";
    return "Idle";
  }

  async function save(t: Task, patch: TaskPatch): Promise<boolean> {
    try {
      const next = await taskApi.update(t.id, patch);
      tasks.update((all) => upsert(all, next));
      void refreshTasks();
      return true;
    } catch (e) {
      toastError(e);
      return false;
    }
  }

  async function create() {
    const title = draft.trim();
    if (!title) return;
    try {
      const t = await taskApi.create(title, projectFor($scope));
      draft = "";
      tasks.update((all) => upsert(all, t));
      void refreshTasks();
    } catch (e) {
      toastError(e);
    }
  }

  function toggleDone(t: Task) {
    void save(t, { status: t.status === "done" ? "todo" : "done" });
  }

  function setStatus(t: Task, status: TaskStatus) {
    if (t.status !== status) void save(t, { status });
  }

  function startEdit(t: Task) {
    editing = t.id;
    editText = t.title;
  }

  async function commitEdit(t: Task) {
    if (editing !== t.id) return;
    editing = "";
    const title = editText.trim();
    if (title && title !== t.title) await save(t, { title });
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  async function reorder(ids: string[]) {
    try {
      await taskApi.reorder(ids);
      await refreshTasks();
    } catch (e) {
      toastError(e);
    }
  }

  function onCardKey(e: KeyboardEvent, t: Task) {
    if (e.key === "F2") {
      e.preventDefault();
      startEdit(t);
      return;
    }
    if (!e.altKey) return;
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const to = nextStatus(t.status, e.key === "ArrowRight" ? 1 : -1);
      if (to) setStatus(t, to);
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const ids = moved(cols[columnOf(t.status)], t.id, e.key === "ArrowUp" ? -1 : 1);
      if (ids) void reorder(ids).then(() => focusCard(t.id));
    }
  }

  async function focusCard(id: string) {
    await tick();
    document.querySelector<HTMLElement>(`[data-task="${id}"] .title`)?.focus();
  }

  function onDragStart(e: DragEvent, t: Task) {
    dragId = t.id;
    e.dataTransfer?.setData("text/plain", t.id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function onDragOver(e: DragEvent, col: Column, before: string | null) {
    if (!dragId) return;
    e.preventDefault();
    if (dropOn?.col !== col || dropOn.before !== before) dropOn = { col, before };
  }

  async function onDrop(e: DragEvent, col: Column) {
    e.preventDefault();
    const t = $tasks.find((x) => x.id === dragId);
    const before = dropOn?.before ?? null;
    dragId = "";
    dropOn = null;
    if (!t) return;
    // Failed stays failed inside Review; anything else takes the column.
    const status: TaskStatus = col === "review" && t.status === "failed" ? "failed" : col;
    if (status !== t.status && !(await save(t, { status }))) return;
    if (col !== "done") await reorder(dropped(cols[col], t.id, before));
  }

  function moveProject(t: Task, value: string) {
    void save(t, { project: value });
  }

  function addNeed(t: Task, id: string) {
    if (id) void save(t, { needs: [...t.needs, id] });
  }

  function dropNeed(t: Task, id: string) {
    void save(t, { needs: t.needs.filter((n) => n !== id) });
  }

  function saveNotes(t: Task) {
    if (notesDraft !== t.notes) void save(t, { notes: notesDraft });
  }

  async function assign(t: Task) {
    if (!$assignPrefs.model) {
      toast("Pick a model first");
      return;
    }
    assigning = true;
    try {
      const r = await taskApi.assign({ id: t.id, model: $assignPrefs.model, effort: $assignPrefs.effort || "medium", mode, lane });
      toast(r.warning ? `Agent started. ${r.warning}` : "Agent started on the task");
      await refreshTasks();
    } catch (e) {
      toastError(e);
    } finally {
      assigning = false;
    }
  }

  async function remove(t: Task) {
    const ok = await ask(`Delete "${t.title}"?`, { title: "Delete task", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await taskApi.remove(t.id);
      if (selected === t.id) selected = "";
      await refreshTasks();
    } catch (e) {
      toastError(e);
    }
  }

  function ago(ms: number) {
    const s = (Date.now() - ms) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }

  function titleOf(id: string) {
    return $tasks.find((t) => t.id === id)?.title ?? id;
  }
</script>

<div class="tasks">
  <header class="bar">
    <select class="scope" aria-label="Project" bind:value={$scope}>
      {#each options as o (o.value)}
        <option value={o.value}>{o.label}{o.count ? ` (${o.count})` : ""}</option>
      {/each}
    </select>
    <form class="new" on:submit|preventDefault={create}>
      <Icon name="plus" />
      <input aria-label="New task" placeholder="New task in {scopeName === 'All projects' ? 'the Inbox' : scopeName}" bind:value={draft} />
    </form>
    <span class="keys">Alt+arrows move a task, F2 renames</span>
  </header>

  {#if $tasksError}
    <p class="err">Couldn't load tasks: {$tasksError}</p>
  {/if}

  <div class="body">
    <div class="board">
      {#each COLUMNS as col (col.id)}
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <section
          class="col"
          class:drop={dropOn?.col === col.id}
          aria-labelledby="col-{col.id}"
          on:dragover={(e) => onDragOver(e, col.id, null)}
          on:drop={(e) => onDrop(e, col.id)}
        >
          <h3 id="col-{col.id}">{col.label}<span class="n">{cols[col.id].length}</span></h3>
          <div class="cards">
            {#each cols[col.id] as t (t.id)}
              {@const agent = agentState(t)}
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <div
                class="card"
                class:on={selected === t.id}
                class:dragging={dragId === t.id}
                class:before={dropOn?.before === t.id}
                data-task={t.id}
                draggable={editing !== t.id}
                on:dragstart={(e) => onDragStart(e, t)}
                on:dragend={() => {
                  dragId = "";
                  dropOn = null;
                }}
                on:dragover|stopPropagation={(e) => onDragOver(e, col.id, t.id)}
              >
                <button
                  class="check"
                  class:done={t.status === "done"}
                  role="checkbox"
                  aria-checked={t.status === "done"}
                  use:tooltip={t.status === "done" ? "Mark not done" : "Mark done"}
                  on:click={() => toggleDone(t)}
                >
                  {#if t.status === "done"}<Icon name="check" size={12} />{/if}
                </button>
                <div class="main">
                  {#if editing === t.id}
                    <input
                      class="title-in"
                      aria-label="Task title"
                      bind:value={editText}
                      use:focusOnMount
                      on:keydown={(e) => {
                        if (e.key === "Enter") void commitEdit(t);
                        else if (e.key === "Escape") {
                          e.stopPropagation();
                          editing = "";
                        }
                      }}
                      on:blur={() => void commitEdit(t)}
                    />
                  {:else}
                    <button
                      class="title"
                      class:struck={t.status === "done"}
                      aria-expanded={selected === t.id}
                      on:click={() => (selected = selected === t.id ? "" : t.id)}
                      on:dblclick={() => startEdit(t)}
                      on:keydown={(e) => onCardKey(e, t)}>{t.title}</button
                    >
                  {/if}
                  {#if $scope === ALL || t.status === "failed" || t.blocked_by.length || agent}
                    <div class="meta">
                      {#if t.status === "failed"}<span class="pill bad">Failed</span>{/if}
                      {#if agent}<span class="pill {agent.cls}">{agent.label}</span>{/if}
                      {#if $scope === ALL}<span class="proj">{projectLabel(t.project, titles)}</span>{/if}
                      {#if t.blocked_by.length && t.status !== "done"}
                        <span class="wait"><Icon name="blocked" size={12} />{waitsOn(t, $tasks)}</span>
                      {/if}
                    </div>
                  {/if}
                </div>
              </div>
            {:else}
              <p class="empty">
                {#if col.id === "todo"}Nothing to do. Add a task above, or ask an agent to plan with tasks.{:else if col.id === "doing"}Assign a task to an agent and it shows here while it works.{:else if col.id === "review"}Finished agent work lands here for you to check.{:else}Nothing done yet.{/if}
              </p>
            {/each}
          </div>
        </section>
      {/each}
    </div>

    {#if sel}
      {@const s = sel}
      {@const session = sessionOf(s)}
      <aside class="detail" aria-label="Task details">
        <div class="d-head">
          <span class="d-id">{s.id}</span>
          <span class="d-when">Added {ago(s.created)} by {s.created_by === "you" ? "you" : "an agent"}</span>
          <button class="icon-btn" use:tooltip={"Close details"} on:click={() => (selected = "")}><Icon name="close" /></button>
        </div>
        <div class="d-scroll">
          <input
            class="d-title"
            aria-label="Title"
            value={s.title}
            on:change={(e) => {
              const v = e.currentTarget.value.trim();
              if (v && v !== s.title) void save(s, { title: v });
            }}
          />

          <div class="field">
            <span class="label" id="task-status">Status</span>
            <div class="seg" role="group" aria-labelledby="task-status" data-ui-segmented>
              {#each STATUSES as st (st.id)}
                <button class:on={s.status === st.id} aria-pressed={s.status === st.id} on:click={() => setStatus(s, st.id)}>{st.label}</button>
              {/each}
            </div>
            {#if s.blocked_by.length && s.status !== "done"}<p class="hint warn">{waitsOn(s, $tasks)}. It can't be done before that.</p>{/if}
          </div>

          <div class="field assign">
            <span class="label">Agent</span>
            {#if session}
              <div class="agent">
                <span class="pill {isLiveSession(session, running) ? 'live' : session.status === 'queued' ? 'queue' : 'idle'}">{sessionLabel(session)}</span>
                <span class="agent-title">{session.title || "Session"}</span>
                <button class="btn" on:click={() => dispatch("openSession", { id: session.id })}>Open session</button>
              </div>
            {:else if s.session}
              <p class="hint">The session that worked on this was deleted.</p>
            {/if}
            <div class="assign-row">
              <ModelPicker bind:value={$assignPrefs.model} bind:effort={$assignPrefs.effort} board={$providers} />
              <select aria-label="Permissions" value={mode} on:change={(e) => assignPrefs.update((p) => ({ ...p, mode: e.currentTarget.value }))}>
                {#each PERMS as p (p.id)}
                  <option value={p.id}>{p.label}</option>
                {/each}
              </select>
              <select aria-label="Lane" bind:value={lane}>
                <option value="build">Build</option>
                <option value="work">Work</option>
              </select>
            </div>
            {#if blockedMode}<p class="hint warn">{blockedMode}</p>{/if}
            <button class="btn primary go" disabled={assigning || s.status === "done" || !$assignPrefs.model} on:click={() => assign(s)}>
              <Icon name="play" />{assigning ? "Starting" : s.session ? "Start a new agent" : "Start an agent"}
            </button>
            {#if s.status === "done"}<p class="hint">Done tasks aren't assigned. Move it back to To do first.</p>{/if}
          </div>

          <label class="field">
            <span class="label">Notes</span>
            <textarea
              rows="6"
              placeholder="What done looks like, links, decisions (markdown)"
              bind:value={notesDraft}
              on:focus={() => (notesFocused = true)}
              on:blur={() => {
                notesFocused = false;
                saveNotes(s);
              }}
            />
          </label>

          <div class="field">
            <span class="label">Needs first</span>
            {#each s.needs as n (n)}
              <div class="need">
                <span class="dot" class:ok={$tasks.find((x) => x.id === n)?.status === "done"} />
                <span class="need-title">{titleOf(n)}</span>
                <button class="icon-btn" use:tooltip={"Remove this need"} on:click={() => dropNeed(s, n)}><Icon name="close" size={12} /></button>
              </div>
            {/each}
            <select
              aria-label="Add a task it needs"
              value=""
              disabled={!candidates.length}
              on:change={(e) => {
                addNeed(s, e.currentTarget.value);
                e.currentTarget.value = "";
              }}
            >
              <option value="">{candidates.length ? "Add a task it needs" : "No other tasks in this project"}</option>
              {#each candidates as c (c.id)}
                <option value={c.id}>{c.title}</option>
              {/each}
            </select>
          </div>

          <label class="field">
            <span class="label">Project</span>
            <select value={s.project ?? ""} on:change={(e) => moveProject(s, e.currentTarget.value)}>
              <option value="">Inbox</option>
              {#each options.filter((o) => o.value !== ALL && o.value !== "") as o (o.value)}
                <option value={o.value}>{o.label}</option>
              {/each}
            </select>
          </label>
        </div>
        <div class="d-foot">
          <span class="hint">{statusLabel(s.status)}, updated {ago(s.updated)}</span>
          <button class="btn danger" on:click={() => remove(s)}><Icon name="trash" />Delete</button>
        </div>
      </aside>
    {/if}
  </div>
</div>

<style>
  .tasks {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--line);
  }
  select {
    height: 32px;
    padding: 0 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    cursor: pointer;
  }
  .scope {
    flex: none;
    max-width: 220px;
  }
  .new {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 12px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .new input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .keys {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  @media (max-width: 1100px) {
    .keys {
      display: none;
    }
  }
  .err {
    margin: 8px 20px 0;
    font-size: var(--fs-md);
    color: var(--bad);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .board {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: repeat(4, minmax(150px, 1fr));
    gap: 12px;
    padding: 12px 16px 16px;
    overflow-x: auto;
  }
  .col {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
    background: var(--hover);
  }
  .col.drop {
    border-color: var(--line);
    background: var(--selected);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 2px 4px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .n {
    font-size: var(--fs-md);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .cards {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
  }
  .card {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    cursor: grab;
  }
  .card.on {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
  }
  .card.dragging {
    opacity: 0.5;
  }
  .card.before {
    box-shadow: 0 -2px 0 var(--accent);
  }
  .check {
    flex: none;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--on-ok);
    cursor: pointer;
  }
  .check::before {
    content: "";
    position: absolute;
    width: 14px;
    height: 14px;
    border: 1px solid var(--muted);
    border-radius: var(--radius-sm);
  }
  .check {
    position: relative;
  }
  .check.done::before {
    background: var(--ok);
    border-color: var(--ok);
  }
  .check :global(svg) {
    position: relative;
  }
  .check:hover {
    background: var(--hover);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .title {
    min-height: 24px;
    padding: 2px 0;
    background: none;
    border: none;
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1.45;
    text-align: left;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .title.struck {
    color: var(--muted);
    text-decoration: line-through;
  }
  .title-in {
    width: 100%;
    height: 26px;
    padding: 0 6px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--fs-md);
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .proj {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .wait {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .pill {
    flex: none;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: var(--fs-xs);
    font-weight: 600;
  }
  .pill.live {
    background: color-mix(in srgb, var(--ok) 16%, transparent);
    color: var(--ok);
  }
  .pill.queue {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }
  /* Red text on a red tint is too faint: the tint says it, the text reads. */
  .pill.bad {
    background: color-mix(in srgb, var(--bad) 24%, transparent);
    color: var(--text);
  }
  .pill.idle {
    background: var(--selected);
    color: var(--muted);
  }
  .empty {
    margin: 4px;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .detail {
    flex: none;
    width: 340px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
  }
  .d-head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 6px 20px;
  }
  .d-id {
    flex: none;
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .d-when {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .icon-btn {
    flex: none;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .d-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 4px 20px 16px;
  }
  .d-title {
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-lg);
    font-weight: 600;
  }
  .d-title:hover {
    border-color: var(--line);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .label {
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .seg {
    display: flex;
    padding: 2px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    gap: 2px;
  }
  .seg button {
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 4px;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--line);
    color: var(--text);
  }
  textarea {
    width: 100%;
    padding: 8px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1.5;
    resize: vertical;
  }
  .need {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-md);
  }
  .need-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warn);
  }
  .dot.ok {
    background: var(--ok);
  }
  .agent {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .agent-title {
    flex: 1 1 120px;
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: var(--fs-md);
  }
  .assign-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .assign-row select {
    height: 28px;
  }
  .go {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    align-self: flex-start;
  }
  .hint {
    margin: 0;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .hint.warn {
    color: var(--warn);
  }
  .d-foot {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 20px;
    border-top: 1px solid var(--line);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .btn.danger:hover {
    color: var(--bad);
    border-color: var(--bad);
  }
</style>
