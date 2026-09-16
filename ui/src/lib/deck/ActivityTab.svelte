<script lang="ts">
  import type { JournalKind, JournalLine } from "../api";

  export let journal: JournalLine[] = [];
  /**
   * STATUS.md as `project_status` renders it: board + journal → text, no
   * model (PLAN.md §15.7). It is the summary above the journal and the
   * header agent's input, so both read the same words.
   */
  export let status = "";

  /**
   * The journal is the body of this tab, so STATUS.md is folded away while
   * there is a journal to read and opens by itself when there is not. Once
   * a person has clicked the fold, their choice wins.
   */
  let summaryOpen = false;
  let summaryPinned = false;
  $: if (!summaryPinned) summaryOpen = journal.length === 0;

  function toggleSummary() {
    summaryPinned = true;
    summaryOpen = !summaryOpen;
  }

  /** Collapsed, the card still says the one line a person came for. */
  $: summaryLine =
    status
      .split("\n")
      .map((l) => l.trim())
      .find((l) => l && !l.startsWith("#")) ?? "";

  /** Filter chips: one lane/person, one task, one kind — all optional. */
  let who = "";
  let task = "";
  let kind: JournalKind | "" = "";

  $: people = [...new Set(journal.map((l) => l.who))].sort();
  $: tasks = [...new Set(journal.map((l) => l.task).filter((t): t is string => !!t))].sort();
  $: kinds = [...new Set(journal.map((l) => l.kind))].sort();
  $: rows = journal
    .filter((l) => (!who || l.who === who) && (!task || l.task === task) && (!kind || l.kind === kind))
    .slice()
    .reverse();

  function clock(at: string): string {
    const d = new Date(at);
    return Number.isNaN(d.getTime()) ? at : d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  function toggle(group: "who" | "task" | "kind", value: string) {
    if (group === "who") who = who === value ? "" : value;
    else if (group === "task") task = task === value ? "" : value;
    else kind = kind === value ? "" : (value as JournalKind);
  }
</script>

<div class="activity">
  {#if status.trim()}
    <div class="summary">
      <button class="s-head" aria-expanded={summaryOpen} on:click={toggleSummary}>
        <span class="s-title">Status</span>
        {#if !summaryOpen && summaryLine}<span class="s-peek">{summaryLine}</span>{/if}
        <span class="s-chev">{summaryOpen ? "−" : "+"}</span>
      </button>
      {#if summaryOpen}<pre class="s-body">{status.trim()}</pre>{/if}
    </div>
  {/if}

  <div class="chips">
    {#each people as p (p)}
      <button class="chip" class:on={who === p} on:click={() => toggle("who", p)}>{p}</button>
    {/each}
    {#each tasks as t (t)}
      <button class="chip task" class:on={task === t} on:click={() => toggle("task", t)}>{t}</button>
    {/each}
    {#each kinds as k (k)}
      <button class="chip kind" class:on={kind === k} on:click={() => toggle("kind", k)}>{k.replace(/_/g, " ")}</button>
    {/each}
  </div>

  {#if !rows.length}
    <div class="empty">{journal.length ? "Nothing matches these filters." : "No activity yet. The journal fills as lanes claim, hand off and ask."}</div>
  {:else}
    <div class="rows">
      {#each rows as line, i (line.at + line.kind + i)}
        <div class="row">
          <span class="at">{clock(line.at)}</span>
          <span class="kind k-{line.kind}">{line.kind.replace(/_/g, " ")}</span>
          <span class="who">{line.who}</span>
          {#if line.task}<span class="task-id">{line.task}</span>{/if}
          <span class="text">{line.text}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .activity { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .summary {
    background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-3);
    padding: 8px 11px 10px; display: flex; flex-direction: column; gap: 6px;
  }
  .s-head {
    display: flex; align-items: center; gap: 8px; background: none; border: none; padding: 0;
    font: inherit; color: var(--text-3); cursor: pointer; text-align: left; width: 100%;
  }
  .s-title { font-size: 11.5px; font-weight: 700; letter-spacing: 0.3px; flex: none; }
  .s-peek {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px; color: var(--text-4);
  }
  .s-chev { font-size: 12px; color: var(--text-4); margin-left: auto; flex: none; }
  .s-body {
    margin: 0; font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11.5px;
    line-height: 1.55; color: var(--text-2); white-space: pre-wrap;
    max-height: 260px; overflow: auto;
  }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip {
    background: var(--surface-1); border: 1px solid transparent; border-radius: var(--radius-pill);
    color: var(--text-3); font: inherit; font-size: 11px; padding: 2px 9px; cursor: pointer;
  }
  .chip:hover { color: var(--text); background: var(--surface-2); }
  .chip.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--text); }
  .chip.task { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; }
  .chip.kind { color: var(--text-4); }
  .chip.kind.on { color: var(--text); }
  .rows { display: flex; flex-direction: column; }
  .row {
    display: flex; align-items: baseline; gap: 8px; padding: 5px 6px; min-width: 0;
    border-bottom: 1px solid var(--line-2); font-size: 12px; color: var(--text-2);
  }
  .row:hover { background: var(--surface-1); }
  .at { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; color: var(--text-4); flex: none; }
  .kind { font-size: 10px; text-transform: uppercase; letter-spacing: 0.4px; color: var(--text-3); flex: none; min-width: 62px; }
  .k-claim, .k-grant { color: var(--ok); }
  .k-deny, .k-block { color: var(--bad); }
  .k-request, .k-convene { color: var(--info); }
  .k-approve, .k-plan_changed { color: var(--warn); }
  .who { font-size: 11.5px; color: var(--text-3); flex: none; }
  .task-id { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; color: var(--accent-text); flex: none; }
  .text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { font-size: 12.5px; color: var(--text-3); padding: 24px 6px; }
</style>
