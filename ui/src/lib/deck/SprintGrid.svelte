<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import TaskCard from "./TaskCard.svelte";
  import type { Plan, Task } from "../api";
  import { laneNames, restingState, type DeckApproval, type TaskLive } from "./state";

  export let plan: Plan;
  /** Task id → live state, derived from the journal by the deck. */
  export let live: Record<string, TaskLive> = {};
  /** Task id → the transfer card waiting on it. */
  export let approvals: Record<string, DeckApproval> = {};

  const dispatch = createEventDispatcher<{ approve: { key: string; allow: boolean } }>();

  $: lanes = laneNames(plan);

  function tasksOf(sprintIndex: number, lane: string): Task[] {
    return plan.sprints[sprintIndex].lanes.find((l) => l.name === lane)?.tasks ?? [];
  }

  function stateOf(t: Task): TaskLive {
    return live[t.id] ?? restingState(t);
  }
</script>

<div class="grid" style="--lanes: {lanes.length}">
  <div class="head corner">sprint</div>
  {#each lanes as lane (lane)}
    <div class="head">lane {lane}</div>
  {/each}

  {#each plan.sprints as sprint, i (sprint.title + i)}
    <div class="sprint">
      <div class="s-title">{sprint.title}</div>
      {#if sprint.target}<div class="s-target">target: {sprint.target}</div>{/if}
    </div>
    {#each lanes as lane (lane)}
      <div class="cell">
        {#each tasksOf(i, lane) as task (task.id)}
          <TaskCard
            {task}
            live={stateOf(task)}
            approval={approvals[task.id] ?? null}
            on:approve={(e) => dispatch("approve", e.detail)}
          />
        {/each}
      </div>
    {/each}
  {/each}
</div>

<style>
  /* Fixed lane width, so the row is measurable: PlanTab compares this grid's
     width with the viewport's to decide whether to show the scroll hint. */
  .grid {
    display: grid;
    grid-template-columns: 148px repeat(var(--lanes), 236px);
    gap: 8px;
    align-items: start;
  }
  .head {
    font-size: 11px; color: var(--text-3); text-transform: lowercase; letter-spacing: 0.3px;
    padding: 2px 4px 4px; border-bottom: 1px solid var(--line-2);
  }
  .corner { color: var(--text-4); }
  .sprint {
    padding: 8px 4px; display: flex; flex-direction: column; gap: 2px;
  }
  /* The sprint column stays put while the lanes scroll past it — a lane you
     scrolled to says nothing without its row label. The negative margin lets
     it cover the 8px grid gap, so no card shows through the seam. */
  .corner, .sprint {
    position: sticky; left: 0; z-index: 1; background: var(--stage);
    margin-right: -8px; padding-right: 12px; box-shadow: 1px 0 0 var(--line-2);
  }
  .s-title { font-size: 12.5px; font-weight: 700; color: var(--text); line-height: 1.3; }
  .s-target { font-size: 11px; color: var(--text-3); }
  .cell { display: flex; flex-direction: column; gap: 8px; min-width: 0; padding: 4px 0; }
</style>
