<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Check } from "../api";
  import Icon from "../Icon.svelte";
  import { check as checkUpdates, dlDone, dlTotal, installUpdate, updateMsg, updateState, updateVersion } from "../updateStore";
  import { clearFinishedSessions } from "../sessions";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  const SHORTCUTS = [
    ["Ctrl+T", "New tab"],
    ["Ctrl+W", "Close tab"],
    ["Ctrl+K", "Find a session or command"],
    ["Ctrl+Tab", "Next tab"],
    ["Ctrl+1 / Ctrl+2", "Ask the agent / search the web"],
    ["Ctrl+J", "Session column"],
    ["Ctrl+B", "Brain"],
    ["Ctrl+H", "History"],
    ["Ctrl+,", "Settings"],
    ["Esc", "Stop the run"],
  ];

  let version = "";
  let checks: Check[] | null = null;
  let checking = true;
  let slow = false;
  let sessions = 0;

  $: failing = (checks ?? []).filter((c) => !c.ok);
  $: pct = $dlTotal > 0 ? Math.min(100, Math.round(($dlDone / $dlTotal) * 100)) : 0;

  // Health checks never hold the section: after a few seconds the rest
  // shows and the checks say they were slow.
  async function runChecks() {
    checking = true;
    slow = false;
    let timer = 0;
    try {
      const late = new Promise<never>((_, reject) => {
        timer = window.setTimeout(() => reject(new Error("slow")), 12000);
      });
      checks = await Promise.race([api.runDoctorQuick(), late]);
    } catch {
      slow = true;
      checks = [];
    } finally {
      clearTimeout(timer);
      checking = false;
    }
  }

  async function countSessions() {
    sessions = (await api.listThreads().catch(() => [])).length;
  }

  onMount(() => {
    api.appVersion().then((v) => (version = v)).catch(() => {});
    void countSessions();
    void runChecks();
  });

  function copyDiagnostics() {
    const text = [`Parzi ${version}`, ...(checks ?? []).map((c) => `${c.ok ? "ok  " : "FAIL"} ${c.name}: ${c.detail}`)].join("\n");
    navigator.clipboard.writeText(text).then(
      () => notify("Copied"),
      (e) => notify(`Could not copy: ${e}`, true),
    );
  }

  // Same flow as History's: asks first, keeps running sessions, closes
  // the tabs of the ones it deletes.
  async function clearSessions() {
    try {
      await clearFinishedSessions();
      await countSessions();
    } catch (e) {
      notify(`Could not clear sessions: ${e}`, true);
    }
  }
</script>

<div class="pref-section">
  <h3 class="section-title">Updates</h3>
  <div class="field-card">
    <div class="field-info">
      <span class="field-label">Parzi {version}</span>
      {#if $updateState === "error" || $updateState === "uptodate" || $updateState === "ready"}
        <span class="field-hint" class:bad={$updateState === "error"}>{$updateState === "error" ? "Couldn't check for updates." : $updateMsg}</span>
      {/if}
    </div>
    {#if $updateState === "ready"}
      <button class="sbtn" on:click={installUpdate}>Install {$updateVersion}</button>
    {:else if $updateState === "available" || $updateState === "downloading"}
      <span class="progress-text">Downloading {$updateVersion}{pct ? ` · ${pct}%` : "…"}</span>
    {:else}
      <button class="sbtn" disabled={$updateState === "checking"} on:click={() => checkUpdates(true)}>
        {$updateState === "checking" ? "Checking…" : "Check for updates"}
      </button>
    {/if}
  </div>
</div>

<div class="pref-section">
  <div class="section-head-with-action">
    <h3 class="section-title">Health</h3>
    <button class="sbtn" disabled={checking} on:click={copyDiagnostics}><Icon name="copy" size={12} /><span>Copy details</span></button>
  </div>
  <div class="checks">
    {#if checking}
      <div class="check"><span class="icon"><Icon name="clock" size={12} /></span><span>Checking…</span></div>
    {:else if slow}
      <div class="check">
        <span class="icon bad"><Icon name="alert" size={12} /></span>
        <span class="grow">The checks took too long.</span>
        <button class="link" on:click={runChecks}>Run again</button>
      </div>
    {:else if !failing.length}
      <div class="check"><span class="icon ok"><Icon name="checkCircle" size={12} /></span><span>All good</span></div>
    {:else}
      {#each failing as c (c.name)}
        <div class="check">
          <span class="icon bad"><Icon name="xCircle" size={12} /></span>
          <span class="grow">{c.detail}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<div class="pref-section">
  <h3 class="section-title">Sessions</h3>
  <div class="field-card">
    <span class="field-label">{sessions} {sessions === 1 ? "session" : "sessions"}</span>
    <button class="sbtn" on:click={clearSessions}><Icon name="trash" size={12} /><span>Clear finished</span></button>
  </div>
</div>

<div class="pref-section">
  <h3 class="section-title">Shortcuts</h3>
  <div class="shortcuts">
    {#each SHORTCUTS as [keys, action]}
      <div class="shortcut"><span>{action}</span><kbd>{keys}</kbd></div>
    {/each}
  </div>
</div>

<style>
  .field-hint.bad {
    color: var(--bad);
  }
  .progress-text {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 14px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 32px;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .icon {
    display: inline-flex;
    flex: none;
    color: var(--muted);
  }
  .icon.ok {
    color: var(--ok);
  }
  .icon.bad {
    color: var(--bad);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .link {
    flex: none;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--text);
  }
  .shortcuts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 24px;
  }
  .shortcut {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
    border-bottom: 1px solid var(--line);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  kbd {
    flex: none;
    padding: 2px 6px;
    background: var(--line);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-xs);
  }
</style>
