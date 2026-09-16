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

<div class="lanes">
  {#each lanes as lane (lane)}
    {@const n = plan.sprints.reduce((a, _, i) => a + tasksOf(i, lane).length, 0)}
    <section class="lane">
      <header class="lane-h">
        <h2>{lane}</h2>
        <span class="n">{n} task{n === 1 ? "" : "s"}</span>
      </header>
      {#each plan.sprints as sprint, i (sprint.title + i)}
        {@const tasks = tasksOf(i, lane)}
        {#if tasks.length}
          <div class="sprint">{sprint.title}{#if sprint.target}<span>{" · "}{sprint.target}</span>{/if}</div>
          {#each tasks as task (task.id)}
            <TaskCard
              {task}
              live={stateOf(task)}
              approval={approvals[task.id] ?? null}
              on:approve={(e) => dispatch("approve", e.detail)}
            />
          {/each}
        {/if}
      {/each}
    </section>
  {/each}
</div>

<style>
  .lanes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 20px 18px;
    align-items: start;
  }
  .lane { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .lane-h { display: flex; align-items: baseline; gap: 8px; padding: 0 2px; }
  h2 { margin: 0; font-size: 13px; font-weight: 600; color: var(--text); }
  .n { font-size: 11px; color: var(--text-4); }
  .sprint { font-size: 11px; color: var(--text-3); padding: 8px 2px 0; }
  .sprint span { color: var(--text-4); }
</style>
