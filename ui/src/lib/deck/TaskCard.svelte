<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Task } from "../api";
  import type { DeckApproval, TaskLive } from "./state";

  export let task: Task;
  export let live: TaskLive;
  /** Set when a `critical` transfer on this task waits for a person. */
  export let approval: DeckApproval | null = null;

  const dispatch = createEventDispatcher<{
    /** Answer the transfer card; the deck forwards it to `approve_tool`. */
    approve: { key: string; allow: boolean };
  }>();

  function answer(allow: boolean) {
    if (approval) dispatch("approve", { key: approval.key, allow });
  }

  const LABEL: Record<TaskLive["state"], string> = {
    pending: "pending",
    claimed: "claimed",
    running: "running",
    blocked: "blocked",
    done: "done",
  };

  $: state = live?.state ?? (task.done ? "done" : "pending");
  $: scopeList = task.scope.join("\n");
  $: sub = state === "running" && live.tool ? `${live.tool} · turn ${live.turn}` : live?.holder || "";
</script>

<div class="card {state}">
  <div class="top">
    <span class="id">{task.id}</span>
    {#if task.critical}<span class="pill crit" title="Lease transfer here needs a person">critical</span>{/if}
    <span class="spacer" />
    <span class="state">{LABEL[state]}</span>
  </div>

  <div class="title">{task.title}</div>

  <div class="meta">
    <span class="repo" title="Repository">{task.repo}</span>
    <span class="scope" title={scopeList}>{task.scope.length} path{task.scope.length === 1 ? "" : "s"}</span>
    {#if task.after.length}<span class="after" title="Runs after">after {task.after.join(", ")}</span>{/if}
  </div>

  {#if sub}<div class="sub">{sub}</div>{/if}

  {#if live?.requestPending}
    <div class="pill req" title={live.note}>lease request pending</div>
  {/if}

  {#if approval}
    <!-- Same card a tool approval shows (Thread.svelte), same tokens. -->
    <div class="approval-card">
      <div>Allow <b>{approval.name}</b> in lane {approval.lane}?</div>
      {#if approval.detail}<pre>{approval.detail}</pre>{/if}
      <div class="row">
        <button class="btn primary" title="approve_tool — allow" on:click={() => answer(true)}>Approve</button>
        <button class="btn" title="approve_tool — deny" on:click={() => answer(false)}>Deny</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .card {
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-left: 2px solid var(--line-3);
    border-radius: var(--radius-3);
    padding: 9px 11px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .card.claimed { border-left-color: var(--info); }
  .card.running { border-left-color: var(--ok); background: var(--ok-soft); }
  .card.blocked { border-left-color: var(--bad); background: var(--bad-soft); }
  .card.done { opacity: 0.62; }
  .top { display: flex; align-items: center; gap: 6px; }
  .id { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px; color: var(--text-3); }
  .spacer { flex: 1; }
  .state { font-size: 10px; color: var(--text-3); text-transform: uppercase; letter-spacing: 0.4px; }
  .card.running .state { color: var(--ok); }
  .card.blocked .state { color: var(--bad); }
  .card.claimed .state { color: var(--info); }
  .title { font-size: 12.5px; font-weight: 600; color: var(--text); line-height: 1.3; }
  .card.done .title { text-decoration: line-through; }
  .meta { display: flex; flex-wrap: wrap; gap: 5px; font-size: 10.5px; color: var(--text-3); }
  .repo, .scope, .after {
    background: var(--surface-2); border-radius: var(--radius-1); padding: 1px 6px; white-space: nowrap;
  }
  .scope { cursor: help; }
  .sub { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; color: var(--text-3); }
  .pill {
    align-self: flex-start; font-size: 10px; border-radius: var(--radius-pill); padding: 1px 7px;
  }
  .pill.crit { color: var(--warn); background: var(--warn-soft); border: 1px solid var(--warn-line); }
  .pill.req { color: var(--info); background: var(--info-soft); border: 1px solid var(--info-line); }
  .approval-card {
    background: var(--warn-soft); border: 1px solid var(--warn-line); border-radius: var(--radius-2);
    padding: 9px 10px; font-size: 12px; color: var(--text); display: flex; flex-direction: column; gap: 8px;
  }
  .approval-card pre {
    background: var(--input); margin: 0; padding: 6px 8px; border-radius: var(--radius-1);
    font-size: 10.5px; max-height: 120px; overflow: auto; white-space: pre-wrap;
  }
  .row { display: flex; gap: 6px; }
  .btn {
    background: var(--surface-2); border: 1px solid var(--line-2); color: var(--text);
    font: inherit; font-size: 11.5px; padding: 4px 11px; border-radius: var(--radius-1); cursor: pointer;
  }
  .btn:hover { background: var(--surface-3); }
  .btn.primary { background: var(--accent-soft); border-color: var(--accent-line); color: var(--accent-text); }
</style>
