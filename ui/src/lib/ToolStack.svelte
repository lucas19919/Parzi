<script lang="ts">
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { renderMarkdown } from "./md";

  /** One tool row: history rows carry output, live rows are status-only. */
  interface StackTool {
    id: string;
    name: string;
    label: string;
    running: boolean;
    ok?: boolean;
    ms?: number;
    output?: string;
  }

  export let tools: StackTool[] = [];
  /** Stacks start collapsed so a 10-tool turn is one line; single tools
      render as their own row (no group chrome). */
  export let open = false;

  const spring = { duration: 160, easing: cubicOut };
  let openOutputs: Set<string> = new Set();

  function toggleStack() {
    open = !open;
  }

  function toggleOutput(id: string) {
    openOutputs = new Set(openOutputs);
    if (openOutputs.has(id)) openOutputs.delete(id);
    else openOutputs.add(id);
  }

  function toolIcon(name: string): string {
    if (name.startsWith("fs.")) return "file";
    if (name.startsWith("shell")) return "term";
    if (name.startsWith("ui.")) return "eye";
    return "globe";
  }

  function fmtMs(ms: number): string {
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`;
  }

  function totalMs(): number {
    return tools.reduce((a, t) => a + (t.ms ?? 0), 0);
  }

  $: doneCount = tools.filter((t) => !t.running && t.ok !== false).length;
  $: failCount = tools.filter((t) => !t.running && t.ok === false).length;
  $: runningCount = tools.filter((t) => t.running).length;
  $: summary = runningCount
    ? `${runningCount} running…`
    : `${doneCount} ok${failCount ? ` · ${failCount} failed` : ""} · ${fmtMs(totalMs())}`;
</script>

{#if tools.length > 1}
  <div class="tool-stack" transition:fade|local={spring}>
    <button class="stack-head" on:click={toggleStack} title={open ? "Collapse tool stack" : "Expand tool stack"}>
      <span class="stack-ico">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M12 2l9 5-9 5-9-5z" /><path d="M3 12l9 5 9-5M3 17l9 5 9-5" /></svg>
      </span>
      <span class="stack-count">{tools.length} tools</span>
      <span class="stack-meta">{summary}</span>
      <span class="tool-caret">{open ? "▾" : "▸"}</span>
    </button>
    {#if open}
      <div class="stack-body">
        {#each tools as t (t.id)}
          <div class="tool-card row" class:running={t.running} class:bad={!t.running && t.ok === false}>
            <span class="tool-ico {toolIcon(t.name)}">
              {#if toolIcon(t.name) === "file"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></svg>
              {:else if toolIcon(t.name) === "term"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 17l6-5-6-5M12 19h8" /></svg>
              {:else if toolIcon(t.name) === "eye"}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
              {:else}
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c3 3.5 3 14 0 18M12 3c-3 3.5-3 14 0 18" /></svg>
              {/if}
            </span>
            <span class="tool-label" title={t.name}>{t.label}</span>
            <span class="tool-sub">{t.name}</span>
            {#if t.running}
              <span class="tool-pill run"><span class="pulse-dot" />running</span>
            {:else}
              <button class="tool-pill {t.ok === false ? 'bad' : 'ok'}" on:click={() => toggleOutput(t.id)}>
                {t.ok === false ? "✗" : "✓"} {fmtMs(t.ms ?? 0)}
              </button>
              {#if t.output}
                <button class="tool-caret" on:click={() => toggleOutput(t.id)}>
                  {openOutputs.has(t.id) ? "▾" : "▸"}
                </button>
              {/if}
            {/if}
          </div>
          {#if !t.running && t.output && openOutputs.has(t.id)}
            <div class="tool-output-box" transition:fade|local={spring}>
              {@html renderMarkdown("```\n" + t.output.slice(0, 6000) + "\n```")}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
{:else if tools.length === 1}
  {#each tools.slice(0, 1) as t (t.id)}
  <div class="tool-card row" class:running={t.running} class:bad={!t.running && t.ok === false} transition:fade|local={spring}>
    <span class="tool-ico {toolIcon(t.name)}">
      {#if toolIcon(t.name) === "file"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></svg>
      {:else if toolIcon(t.name) === "term"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 17l6-5-6-5M12 19h8" /></svg>
      {:else if toolIcon(t.name) === "eye"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
      {:else}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c3 3.5 3 14 0 18M12 3c-3 3.5-3 14 0 18" /></svg>
      {/if}
    </span>
    <span class="tool-label" title={t.name}>{t.label}</span>
    <span class="tool-sub">{t.name}</span>
    {#if t.running}
      <span class="tool-pill run"><span class="pulse-dot" />running</span>
    {:else}
      <button class="tool-pill {t.ok === false ? 'bad' : 'ok'}" on:click={() => toggleOutput(t.id)}>
        {t.ok === false ? "✗" : "✓"} {fmtMs(t.ms ?? 0)}
      </button>
      {#if t.output}
        <button class="tool-caret" on:click={() => toggleOutput(t.id)}>
          {openOutputs.has(t.id) ? "▾" : "▸"}
        </button>
      {/if}
    {/if}
  </div>
  {#if !t.running && t.output && openOutputs.has(t.id)}
    <div class="tool-output-box" transition:fade|local={spring}>
      {@html renderMarkdown("```\n" + t.output.slice(0, 6000) + "\n```")}
    </div>
  {/if}
  {/each}
{/if}

<style>
  .tool-stack {
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 3px;
    max-width: fit-content;
    min-width: 280px;
  }
  .stack-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-2);
    font: inherit;
    font-size: 12px;
    padding: 5px 8px;
    cursor: pointer;
    text-align: left;
  }
  .stack-head:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .stack-ico {
    display: flex;
    align-items: center;
    color: var(--text-3);
  }
  .stack-count {
    font-weight: 600;
    color: var(--text);
  }
  .stack-meta {
    flex: 1;
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 10.5px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stack-body {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 3px;
  }
  .tool-card {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 12px;
    max-width: fit-content;
  }
  .tool-card.row {
    max-width: none;
  }
  .stack-body .tool-card {
    background: var(--surface-2);
  }
  .tool-card.bad {
    border-color: var(--bad-line);
    background: var(--bad-soft);
  }
  .tool-ico {
    display: flex;
    align-items: center;
    color: var(--text-3);
    flex: none;
  }
  .tool-label {
    font-size: 12px;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool-sub {
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 10px;
    color: var(--text-3);
    opacity: 0.7;
    flex: none;
  }
  .tool-pill {
    background: var(--surface-2);
    border: none;
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 10px;
    font-family: var(--parzi-mono), ui-monospace, monospace;
    color: var(--text-3);
    cursor: pointer;
    flex: none;
  }
  .stack-body .tool-pill {
    background: var(--surface-3);
  }
  .tool-pill.ok {
    color: var(--ok);
    background: var(--ok-soft);
  }
  .tool-pill.bad {
    color: var(--bad);
    background: var(--bad-soft);
  }
  .tool-pill.run {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--accent);
    cursor: default;
  }
  .pulse-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.1); }
  }
  .tool-card.running {
    border-color: var(--accent-mid);
  }
  .tool-caret {
    background: transparent;
    border: none;
    color: var(--text-3);
    cursor: pointer;
    font-size: 12px;
    padding: 0 2px;
    flex: none;
  }
  .tool-output-box {
    margin-top: 2px;
    font-size: 11px;
  }
</style>
