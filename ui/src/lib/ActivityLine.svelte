<script lang="ts">
  import ToolStack from "./ToolStack.svelte";

  interface ActivityTool {
    id: string;
    name: string;
    label: string;
    running: boolean;
    ok: boolean;
    ms: number;
  }

  /** Live tools for this turn (status-only rows, no output). */
  export let tools: ActivityTool[] = [];
  /** True while prose tokens are arriving. */
  export let prose = false;
  /** Full completed tool history for the expander (same shape). */
  export let history: ActivityTool[] = [];

  let open = false;

  $: running = tools.filter((t) => t.running);
  $: lastRunning = running.length ? running[running.length - 1] : null;
  $: doneCount = tools.filter((t) => !t.running && t.ok !== false).length
    + history.filter((t) => !t.running && t.ok !== false).length;
  $: failCount = tools.filter((t) => !t.running && t.ok === false).length
    + history.filter((t) => !t.running && t.ok === false).length;
  $: total = tools.length + history.length;
  $: line = lastRunning
    ? `${lastRunning.label}…`
    : prose
      ? "Writing…"
      : tools.length
        ? "Working…"
        : "Starting…";
  $: counts = total
    ? `${doneCount} ok${failCount ? ` · ${failCount} failed` : ""}`
    : "";
  $: expandable = tools.length + history.length > 0;
</script>

{#if tools.length || prose}
  <div class="activity">
    <button class="activity-line" class:static={!expandable} on:click={() => expandable && (open = !open)} title={expandable ? (open ? "Collapse tool activity" : "Expand tool activity") : line}>
      <span class="activity-dot" />
      <span class="activity-text">{line}</span>
      {#if counts}<span class="activity-counts">{counts}</span>{/if}
      {#if expandable}<span class="activity-caret">{open ? "▾" : "▸"}</span>{/if}
    </button>
    {#if open && expandable}
      <div class="activity-body">
        <ToolStack tools={[...history, ...tools]} open />
      </div>
    {/if}
  </div>
{/if}

<style>
  .activity { align-self: flex-start; max-width: 100%; }
  .activity-line {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    background: transparent;
    border: none;
    border-radius: 999px;
    padding: 4px 2px;
    font: inherit;
    font-size: 12px;
    color: var(--text-3);
    cursor: pointer;
    text-align: left;
  }
  .activity-line.static { cursor: default; }
  .activity-dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .activity-dot { animation: none; }
  }
  .activity-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .activity-counts {
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 10.5px;
    opacity: 0.75;
    flex: none;
  }
  .activity-caret { font-size: 11px; opacity: 0.7; flex: none; }
  .activity-body { margin-top: 4px; min-width: 280px; }
</style>
