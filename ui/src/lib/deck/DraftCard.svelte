<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Draft } from "../api";

  export let draft: Draft;
  /** True while `project_audit` runs for this draft. */
  export let busy = false;

  const dispatch = createEventDispatcher<{
    /** Hand this draft to the orchestrator (`project_audit`). */
    audit: { draft: Draft };
    /** Read the whole draft in the right deck's Docs tab. */
    open: { draft: Draft };
  }>();

  /** First lines of the body, headings and list markers stripped. */
  $: preview = draft.content
    .split("\n")
    .filter((l) => l.trim() && !/^#/.test(l.trim()))
    .slice(0, 3)
    .join(" ")
    .replace(/^[-*]\s+/gm, "")
    .slice(0, 240);
</script>

<div class="draft">
  <div class="top">
    <button class="title" title="Open {draft.path}" on:click={() => dispatch("open", { draft })}>{draft.title || draft.name}</button>
    <span class="name">{draft.name}</span>
  </div>
  <div class="preview">{preview}</div>
  <button class="audit" disabled={busy} title="project_audit — the orchestrator writes PLAN.md from this draft" on:click={() => dispatch("audit", { draft })}>
    {busy ? "Orchestrator reading…" : "Audit with orchestrator"}
  </button>
</div>

<style>
  .draft {
    background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-3);
    padding: 11px 13px; display: flex; flex-direction: column; gap: 7px; min-width: 0;
  }
  .top { display: flex; align-items: baseline; gap: 8px; }
  .title {
    background: none; border: none; padding: 0; cursor: pointer; text-align: left;
    font: inherit; font-size: 13px; font-weight: 600; color: var(--text); min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .title:hover { color: var(--accent-text); }
  .name { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px; color: var(--text-4); flex: none; }
  .preview { font-size: 12px; color: var(--text-3); line-height: 1.5; }
  .audit {
    align-self: flex-start; background: var(--accent-soft); border: 1px solid var(--accent-line);
    color: var(--accent-text); font: inherit; font-size: 11.5px; padding: 4px 11px;
    border-radius: var(--radius-1); cursor: pointer;
  }
  .audit:hover:not(:disabled) { background: var(--accent-mid); }
  .audit:disabled { opacity: 0.6; cursor: default; }
</style>
