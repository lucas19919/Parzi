<script lang="ts">
  // One test run, read from the output of the command that ran it: passed or
  // failed at a glance, which tests failed, and the first failure's message.
  import Icon from "./Icon.svelte";
  import { testHeadline, type TestRun } from "./testRuns";

  export let run: TestRun;
  export let command = "";

  const SHOWN = 3;
  let all = false;

  $: bad = run.failed > 0;
  $: counts = [
    run.passed ? `${run.passed} passed` : "",
    run.failed ? `${run.failed} failed` : "",
    run.skipped ? `${run.skipped} skipped` : "",
  ].filter(Boolean);
  $: listed = all ? run.failures : run.failures.slice(0, SHOWN);
</script>

<div class="tests" class:bad role="status" aria-label={testHeadline(run)}>
  <div class="head">
    <span class="mark" aria-hidden="true"><Icon name={bad ? "alert" : "check"} size={12} /></span>
    <span class="t-label">{testHeadline(run)}</span>
    <span class="meta t-hint">{counts.join(" · ")}</span>
  </div>
  {#if command}<div class="cmd" title={command}>{command}</div>{/if}
  {#if bad && run.failures.length}
    <ul class="fails">
      {#each listed as name (name)}<li title={name}>{name}</li>{/each}
    </ul>
    {#if run.failures.length > SHOWN}
      <button class="more t-hint" on:click={() => (all = !all)}>{all ? "Show fewer" : `${run.failures.length - SHOWN} more`}</button>
    {/if}
  {/if}
  {#if bad && run.firstError}<pre class="err">{run.firstError}</pre>{/if}
</div>

<style>
  .tests {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: calc(var(--space) * 2) 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    background: color-mix(in srgb, var(--panel) var(--card-fill), transparent);
    border: 1px solid color-mix(in srgb, var(--line) var(--card-edge), transparent);
    -webkit-backdrop-filter: var(--card-blur);
    backdrop-filter: var(--card-blur);
    border-radius: var(--radius);
  }
  .head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
  }
  .mark {
    display: inline-flex;
    flex: none;
    color: var(--ok-text);
  }
  .bad .mark {
    color: var(--bad-text);
  }
  .meta {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .cmd {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-sm);
  }
  .fails {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .fails li {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--bad-text);
    font-family: var(--mono);
    font-size: var(--fs-sm);
  }
  .more {
    align-self: flex-start;
    padding: 0;
    background: none;
    border: none;
    cursor: pointer;
  }
  .more:hover {
    color: var(--text);
  }
  .err {
    margin: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    max-height: 160px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--bg);
    border-radius: var(--radius);
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
</style>
