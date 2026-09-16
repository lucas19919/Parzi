<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import SprintGrid from "./SprintGrid.svelte";
  import type { Plan } from "../api";
  import { allTasks, laneNames, type DeckApproval, type TaskLive } from "./state";

  export let plan: Plan;
  export let live: Record<string, TaskLive> = {};
  export let approvals: Record<string, DeckApproval> = {};
  /** Drafting/Planned projects have no plan to show yet. */
  export let status = "drafting";

  const dispatch = createEventDispatcher<{ approve: { key: string; allow: boolean } }>();

  $: tasks = allTasks(plan);
  $: done = tasks.filter((t) => (live[t.id]?.state ?? (t.done ? "done" : "pending")) === "done").length;
  $: lanes = laneNames(plan);
</script>

{#if !plan.sprints.length}
  <div class="empty">
    <span class="e-title">No plan yet</span>
    <span class="e-sub">
      {status === "drafting"
        ? "Ask the header on Dashboard, then send a draft to the orchestrator."
        : "The orchestrator writes PLAN.md when a draft is audited."}
    </span>
  </div>
{:else}
  <div class="plan">
    <div class="bar">
      <span class="count">{done}/{tasks.length} tasks done</span>
      <span class="track"><span class="fill" style="width: {tasks.length ? (done / tasks.length) * 100 : 0}%" /></span>
      <span class="more">{lanes.length} lane{lanes.length === 1 ? "" : "s"}</span>
    </div>
    <SprintGrid {plan} {live} {approvals} on:approve={(e) => dispatch("approve", e.detail)} />
  </div>
{/if}

<style>
  .plan { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .bar { display: flex; align-items: center; gap: 10px; }
  .count { font-size: 11.5px; color: var(--text-3); white-space: nowrap; }
  .track { flex: 1; height: 4px; background: var(--surface-2); border-radius: var(--radius-pill); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .more { font-size: 11px; color: var(--text-4); white-space: nowrap; flex: none; }
  .empty {
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px;
    padding: 56px 24px; text-align: center; color: var(--text-4);
  }
  .e-title { font-size: 13px; color: var(--text-3); font-weight: 600; }
  .e-sub { font-size: 12px; max-width: 360px; line-height: 1.5; }
</style>
