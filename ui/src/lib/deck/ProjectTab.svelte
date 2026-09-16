<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Thread from "../Thread.svelte";
  import AskBox from "./AskBox.svelte";
  import DraftCard from "./DraftCard.svelte";
  import { renderMarkdown } from "../md";
  import type { AuditResult, ChatEvent, Draft, Project } from "../api";
  import { projectMarkdown } from "./state";

  export let project: Project;
  export let drafts: Draft[] = [];
  /** The orchestrator's answer to `project_audit`, waiting for Approve. */
  export let audit: AuditResult | null = null;
  /** Name of the draft currently being audited ("" = none). */
  export let auditing = "";
  export let approving = false;
  /** Compact thread of the role you are talking to. */
  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveTools: { id: string; name: string; label: string; running: boolean; ok: boolean; ms: number }[] = [];
  export let streaming = false;
  export let canDirect = true;

  const dispatch = createEventDispatcher<{
    ask: { prompt: string; to: "header" | "orchestrator" };
    audit: { draft: Draft };
    approve: void;
    openDraft: { draft: Draft };
  }>();

  /** PROJECT.md opens at Why/What; the rest of the file is one click away. */
  let docOpen = false;

  $: doc = renderMarkdown(projectMarkdown(project, docOpen));
  $: status = project.status;
  $: hasDoc = !!(project.why.trim() || project.what.length);
  $: hasDrafts = drafts.length > 0;
</script>

<div class="tab" class:solo={!hasDrafts}>
  <div class="left">
    {#if audit}
      <div class="summary">
        <div class="s-title">Orchestrator audit</div>
        <button class="approve" disabled={approving} title="project_approve" on:click={() => dispatch("approve")}>
          {approving ? "Starting sprint 1…" : "Approve — start sprint 1"}
        </button>
        <pre class="s-body">{audit.summary}</pre>
        <div class="s-hint">Approve writes status Running and dispatches sprint 1's lanes.</div>
      </div>
    {/if}

    <div class="head">
      <span class="pill s-{status}">{status}</span>
      {#each project.repos as r (r)}<span class="pill dim">{r}</span>{/each}
    </div>

    <div class="askwrap">
      <AskBox
        headerModel={project.roster.header}
        orchestratorModel={project.roster.orchestrator}
        {canDirect}
        busy={streaming}
        on:ask={(e) => dispatch("ask", e.detail)}
      />
    </div>

    {#if hasDoc}
    <div class="doc">
      <div class="prose">{@html doc}</div>
      <button class="more" aria-expanded={docOpen} on:click={() => (docOpen = !docOpen)}>
        {docOpen ? "Show less" : "Roster, critical and constraints"}
      </button>
    </div>
    {/if}

    {#if events.length || streaming}
      <div class="thread">
        <Thread {events} {liveText} {liveTools} {streaming} />
      </div>
    {/if}
  </div>

  {#if hasDrafts}
  <div class="right">
    <div class="drafts">
      <div class="d-title">Drafts</div>
      {#each drafts as d (d.path)}
        <DraftCard
          draft={d}
          busy={auditing === d.name}
          on:audit={(e) => dispatch("audit", e.detail)}
          on:open={(e) => dispatch("openDraft", e.detail)}
        />
      {/each}
    </div>
  </div>
  {/if}
</div>

<style>
  .tab { display: grid; grid-template-columns: minmax(0, 1.5fr) minmax(260px, 0.9fr); gap: 18px; align-items: start; }
  .tab.solo { grid-template-columns: minmax(0, 640px); }
  @media (max-width: 980px) { .tab { grid-template-columns: 1fr; } }
  .left, .right { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .left { padding: 4px 2px 18px; }
  .head { display: flex; flex-wrap: wrap; gap: 5px; }
  .pill {
    font-size: 10.5px; border-radius: var(--radius-pill); padding: 2px 9px;
    background: var(--surface-2); color: var(--text-3); border: 1px solid transparent;
  }
  .pill.dim { color: var(--text-4); }
  .pill.s-running { color: var(--ok); background: var(--ok-soft); border-color: var(--ok-line); }
  .pill.s-drafting { color: var(--warn); background: var(--warn-soft); border-color: var(--warn-line); }
  .pill.s-planned { color: var(--info); background: var(--info-soft); border-color: var(--info-line); }
  .pill.s-done { color: var(--text-3); }
  .pill.s-parked { color: var(--text-4); }

  /* The Ask box and its Direct toggle are the tab's one control, so they
     stay on screen while PROJECT.md and the thread scroll under them. */
  .askwrap { position: sticky; top: 0; z-index: 2; background: var(--stage); padding: 6px 0 2px; }

  .doc { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; min-width: 0; }
  .more {
    background: none; border: none; padding: 0; font: inherit; font-size: 11.5px;
    color: var(--text-4); cursor: pointer; text-decoration: underline;
    text-underline-offset: 3px; text-decoration-color: var(--line-3);
  }
  .more:hover { color: var(--text-3); }

  .prose { font-size: 13px; line-height: 1.6; color: var(--text-2); min-width: 0; }
  .prose :global(h1) { font-size: 20px; margin: 0 0 0.5em; letter-spacing: -0.3px; color: var(--text); }
  .prose :global(h2) { font-size: 15px; margin: 1.2em 0 0.4em; color: var(--text); }
  .prose :global(p) { margin: 0 0 0.7em; }
  .prose :global(ul) { padding-left: 1.3em; margin: 0 0 0.7em; }
  .prose :global(li) { margin: 0.15em 0; }
  .prose :global(code) { font-size: 0.92em; background: var(--surface-2); padding: 1px 5px; border-radius: var(--radius-1); }
  /* The `key: value` header block (md.ts `parzi_front`): rows, not prose. */
  .prose :global(dl.md-front) {
    display: grid; grid-template-columns: max-content minmax(0, 1fr);
    gap: 3px 12px; margin: 0 0 1em; font-size: 12px;
  }
  .prose :global(dl.md-front dt) { color: var(--text-4); }
  .prose :global(dl.md-front dd) { margin: 0; color: var(--text-3); min-width: 0; overflow-wrap: anywhere; }
  /* `- [ ]` / `- [x]` under ## What: the box is the marker, so no bullet. */
  .prose :global(li.task) { list-style: none; margin-left: -1.15em; }
  .prose :global(li.task.done) { color: var(--text-4); }

  .thread { border-top: 1px solid var(--line-2); max-height: 420px; overflow: auto; }
  .thread :global(.thread-col) { padding: 12px 0 24px; max-width: none; }

  .summary {
    background: var(--surface-1); border: 1px solid var(--accent-line); border-radius: var(--radius-3);
    padding: 12px 13px; display: flex; flex-direction: column; gap: 9px;
  }
  .s-title { font-size: 12px; font-weight: 700; color: var(--text); }
  .s-body {
    margin: 0; font-family: inherit; font-size: 12px; line-height: 1.55; color: var(--text-2);
    white-space: pre-wrap; max-height: 160px; overflow: auto;
  }
  .approve {
    align-self: flex-start; background: var(--accent-soft); border: 1px solid var(--accent-line);
    color: var(--accent-text); font: inherit; font-size: 12px; padding: 5px 13px;
    border-radius: var(--radius-1); cursor: pointer;
  }
  .approve:hover:not(:disabled) { background: var(--accent-mid); }
  .approve:disabled { opacity: 0.6; cursor: default; }
  .s-hint { font-size: 11px; color: var(--text-4); }

  .drafts { display: flex; flex-direction: column; gap: 8px; }
  .d-title { font-size: 12px; font-weight: 700; color: var(--text-3); letter-spacing: 0.3px; }
</style>
