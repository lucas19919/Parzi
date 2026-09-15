<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import RosterSelector from "./RosterSelector.svelte";
  import LivingPlanView from "./LivingPlanView.svelte";
  import { api, type LaneView, type ProjectRoster, type SessionMeta } from "./api";

  export let project = "default";
  export let root = "";
  export let branch = "";
  export let lanes: LaneView[] = [];
  export let threads: SessionMeta[] = [];
  export let roster: ProjectRoster | null = null;
  export let plan = "";

  const dispatch = createEventDispatcher<{
    openThread: { id: string };
    newThread: void;
    killRun: { id: string };
    newSubsession: { id: string };
    askHeader: { prompt: string };
    executePlan: void;
    planChanged: { plan: string };
    rosterSaved: { roster: ProjectRoster };
  }>();

  let headerPrompt = "";
  let knowledge = "";
  let savingKnowledge = false;
  let knowledgeLoadedFor = "";

  $: if (project && project !== knowledgeLoadedFor) {
    knowledgeLoadedFor = project;
    api.getProjectKnowledge(project).then((k) => {
      knowledge = k || "";
    }).catch(() => {
      knowledge = "";
    });
  }

  async function saveKnowledge() {
    savingKnowledge = true;
    try {
      await api.saveProjectKnowledge(project, knowledge);
    } finally {
      savingKnowledge = false;
    }
  }

  $: mine = threads.filter((t) => (t.project || "default") === project);
  $: live = mine.filter((t) => t.status === "active" || t.status === "queued").slice(0, 8);

  function ask() {
    if (!headerPrompt.trim()) return;
    dispatch("askHeader", { prompt: headerPrompt.trim() });
    headerPrompt = "";
  }
</script>

<div class="mp">
  <div class="banner">
    <div class="b-row">
      <span class="b-name">{project}</span>
      {#if branch}<span class="b-branch">⎇ {branch}</span>{/if}
      {#if live.length}<span class="b-live"><span class="dot" />{live.length} live</span>{/if}
      <span class="spacer" />
      <button class="go" on:click={() => dispatch("newThread")}>+ New conversation</button>
    </div>
    {#if root}<div class="b-root" title={root}>{root}</div>{/if}
    {#if lanes.length}
      <div class="chips">
        {#each lanes as l}
          <span class="chip">
            {l.name}
            <span class="mode">{l.mode}</span>
            {#if l.isolated_worktree}<span class="wt-chip">worktree</span>{/if}
          </span>
        {/each}
      </div>
    {/if}
  </div>

  <details class="setup">
    <summary>Agents &amp; models</summary>
    <div class="setup-body">
      <RosterSelector {project} {roster} on:saved={(e) => dispatch("rosterSaved", e.detail)} />

      <div class="header-hub">
        <div class="hub-title">Header agent — architecture &amp; planning</div>
        <div class="hub-row">
          <input
            class="hub-in"
            placeholder="Ask the Header agent about architecture, status, or the plan…"
            bind:value={headerPrompt}
            on:keydown={(e) => { if (e.key === "Enter") ask(); }}
          />
          <button class="go" on:click={ask}>Ask</button>
        </div>
        <div class="hub-sub">Answers in a thread using the Header role model. Plan edits sync to the living plan below.</div>
      </div>

      <div class="knowledge-hub">
        <div class="k-head">
          <div class="hub-title">Cumulative Project Knowledge (<code>KNOWLEDGE.md</code>)</div>
          <button class="go-sm" on:click={saveKnowledge} disabled={savingKnowledge}>
            {savingKnowledge ? "Saving…" : "Save Knowledge"}
          </button>
        </div>
        <textarea
          class="k-area"
          rows="4"
          placeholder="Architectural decisions, learned patterns, gotchas... (automatically injected into Header &amp; Worker context)"
          bind:value={knowledge}
        ></textarea>
        <div class="hub-sub">Agents automatically read and contribute learnings here to persist context across sessions.</div>
      </div>
    </div>
  </details>

  <div class="split">
    <div class="col">
      <LivingPlanView
        {project}
        {plan}
        on:execute={() => dispatch("executePlan")}
        on:changed={(e) => dispatch("planChanged", e.detail)}
      />
    </div>
    <div class="col">
      <div class="swarm">
        <div class="swarm-title">Active lane swarm</div>
        {#if !live.length}
          <div class="empty">No live lanes. Execute the plan or start a conversation.</div>
        {:else}
          {#each live as t (t.id)}
            <button class="lane-card" on:click={() => dispatch("openThread", { id: t.id })}>
              <span class="dot" title={t.status} />
              <span class="main">
                <span class="t">{t.title || "untitled"}</span>
                <span class="meta">{t.lane || "no lane"} · {t.model || "no model"} · {t.status}</span>
              </span>
              <span
                class="x" role="button" tabindex="0" title="Stop run"
                on:click|stopPropagation={() => dispatch("killRun", { id: t.id })}
                on:keydown={(e) => e.key === "Enter" && dispatch("killRun", { id: t.id })}>✕</span
              >
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .mp { max-width: 1060px; margin: 0 auto; padding: 24px 28px 100px; display: flex; flex-direction: column; gap: 12px; width: 100%; box-sizing: border-box; }
  .banner { background: var(--surface-1); border: 1px solid var(--line); border-radius: 14px; padding: 16px 18px; display: flex; flex-direction: column; gap: 8px; }
  .b-row { display: flex; align-items: center; gap: 10px; }
  .b-name { font-size: 19px; font-weight: 700; color: var(--text); letter-spacing: -0.3px; }
  .b-branch { font-size: 12px; color: var(--text-3); font-family: var(--parzi-mono), ui-monospace, monospace; }
  .b-live { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; font-weight: 600; color: var(--ok); background: var(--ok-soft); border: 1px solid var(--ok-line); border-radius: 6px; padding: 2px 8px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 8px var(--ok-line); flex: none; }
  .spacer { flex: 1; }
  .go { background: var(--accent-soft); border: 1px solid var(--accent-line); color: var(--accent-text); font: inherit; font-size: 12px; padding: 6px 12px; cursor: pointer; border-radius: 7px; white-space: nowrap; }
  .b-root { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px; color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text); background: var(--surface-2); border: 1px solid var(--line); border-radius: 7px; padding: 4px 9px; }
  .mode { font-size: 10px; color: var(--accent); background: var(--accent-soft); border-radius: 4px; padding: 1px 5px; }
  .wt-chip { font-size: 9.5px; font-family: var(--parzi-mono), monospace; color: var(--ok); background: var(--ok-soft); border: 1px solid var(--ok-line); border-radius: 4px; padding: 1px 5px; }
  .header-hub, .knowledge-hub { background: var(--surface-1); border: 1px solid var(--line); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
  .hub-title { font-size: 12px; font-weight: 700; letter-spacing: 0.3px; color: var(--text); }
  .hub-row { display: flex; gap: 8px; }
  .hub-in { flex: 1; background: var(--surface-2); border: 1px solid var(--line); color: var(--text); border-radius: 8px; padding: 9px 12px; font: inherit; font-size: 13px; }
  .hub-sub { font-size: 11.5px; color: var(--text-3); }
  .k-head { display: flex; align-items: center; justify-content: space-between; }
  .go-sm { background: var(--accent-soft); border: 1px solid var(--accent-line); color: var(--accent-text); font: inherit; font-size: 11px; padding: 4px 10px; cursor: pointer; border-radius: 6px; }
  .k-area { width: 100%; box-sizing: border-box; background: var(--surface-2); border: 1px solid var(--line); color: var(--text); font-family: var(--parzi-mono), monospace; font-size: 12px; border-radius: 8px; padding: 8px 10px; resize: vertical; line-height: 1.4; }
  .setup {
    background: var(--surface-1); border: 1px solid var(--line);
    border-radius: 12px; padding: 4px 14px;
  }
  .setup summary {
    cursor: pointer; font-size: 12px; font-weight: 700; letter-spacing: 0.3px;
    color: var(--text-3); padding: 8px 0; list-style: none;
  }
  .setup summary::-webkit-details-marker { display: none; }
  .setup summary::before { content: "▸ "; font-size: 10px; }
  .setup[open] summary::before { content: "▾ "; }
  .setup summary:hover { color: var(--text); }
  .setup-body { display: flex; flex-direction: column; gap: 12px; padding: 4px 0 12px; }
  .setup-body :global(.roster), .setup-body .header-hub { border: none; padding-left: 0; padding-right: 0; }
  .split { display: grid; grid-template-columns: 1.2fr 1fr; gap: 12px; }
  @media (max-width: 900px) { .split { grid-template-columns: 1fr; } }
  .col { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .swarm { background: var(--surface-1); border: 1px solid var(--line); border-radius: 12px; padding: 14px 16px; display: flex; flex-direction: column; gap: 8px; }
  .swarm-title { font-size: 13px; font-weight: 700; color: var(--text); }
  .empty { font-size: 12.5px; color: var(--text-3); }
  .lane-card { display: flex; align-items: center; gap: 10px; width: 100%; box-sizing: border-box; background: var(--surface-2); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; cursor: pointer; font: inherit; text-align: left; color: var(--text); }
  .lane-card:hover { background: var(--surface-3); }
  .lane-card .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .lane-card .t { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lane-card .meta { font-size: 11px; color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .x { color: var(--text-3); padding: 3px 7px; border-radius: 5px; font-size: 12px; }
  .x:hover { background: var(--surface-1); color: var(--text); }
</style>
