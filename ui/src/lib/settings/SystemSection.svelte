<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Check } from "../api";
  import Icon from "../Icon.svelte";
  import { check as checkUpdatesStore, dlDone, dlTotal, installUpdate, updateMsg, updateState, updateVersion } from "../updateStore";
  import { clearFinishedSessions } from "../sessions";
  import { toast } from "../toast";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  let quick: Check[] | null = null;
  let loaded = false;
  let diagTimedOut = false;

  let threadCount = 0;

  let appVersion = "";

  // Update state lives in updateStore: the startup check pre-downloads
  // in the background, so this section only reflects and installs.
  async function checkUpdates() {
    await checkUpdatesStore(true);
  }

  async function installUpdateClicked() {
    await installUpdate();
  }

  // Diagnostics must never hold the section hostage: provider probes are
  // slow, so a timeout renders the rest and says so.
  async function load() {
    loaded = false;
    diagTimedOut = false;
    let timer = 0;
    try {
      const all = Promise.all([
        api.runDoctorQuick(),
        api.listThreads(),
        api.appVersion().catch(() => ""),
      ]);
      const timeout = new Promise<never>((_, rej) => {
        timer = window.setTimeout(() => rej(new Error("diagnostics timed out")), 12000);
      });
      const [q, threads, v] = await Promise.race([all, timeout]);
      quick = q;
      threadCount = threads.length;
      appVersion = v;
    } catch (e) {
      diagTimedOut = true;
      quick = [];
      try {
        threadCount = (await api.listThreads()).length;
      } catch (e2) {
        notify(`Couldn't count sessions: ${e2}`);
      }
      appVersion = await api.appVersion().catch(() => "");
    } finally {
      clearTimeout(timer);
      loaded = true;
    }
  }

  onMount(() => {
    void load();
  });

  function copyDiagnostics() {
    navigator.clipboard.writeText((quick ?? []).map((c) => `${c.ok ? "PASS" : "FAIL"} [${c.name}] ${c.detail}`).join("\n")).then(
      () => notify("Diagnostics copied to clipboard"),
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
  }

  // Same flow as History's Clear sessions: asks first, keeps running
  // ones, closes the tabs of the sessions it deleted.
  async function purgeSessions() {
    try {
      await clearFinishedSessions();
      threadCount = (await api.listThreads()).length;
    } catch (e) {
      toast(`Purge failed: ${e}`, true);
    }
  }
</script>

{#if !loaded}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
    <div class="skel" />
  </div>
{:else}
  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Updates</h3>
        <p class="section-desc">Parzi {appVersion ? `v${appVersion}` : ""} · stable channel · checks GitHub releases.</p>
      </div>
      {#if $updateState === "ready"}
        <button class="sbtn" on:click={installUpdateClicked}>
          <span>Install v{$updateVersion}</span>
        </button>
      {:else if $updateState === "available" || $updateState === "downloading"}
        <button class="sbtn" disabled>
          <span>{$updateState === "downloading" ? "Downloading…" : "Preparing…"}</span>
        </button>
      {:else}
        <button class="sbtn" disabled={$updateState === "checking"} on:click={checkUpdates}>
          <span>{$updateState === "checking" ? "Checking…" : "Check for updates"}</span>
        </button>
      {/if}
    </div>
    {#if $updateState === "downloading" && $dlTotal > 0}
      <div class="upd-bar"><div class="upd-fill" style={`width:${Math.min(100, Math.round(($dlDone / $dlTotal) * 100))}%`} /></div>
    {/if}
    {#if $updateMsg && $updateState !== "idle"}
      <div class="check-row">
        <span class="check-icon {$updateState === "ready" || $updateState === "uptodate" ? "pass" : $updateState === "error" ? "fail" : ""}">{$updateState === "ready" || $updateState === "uptodate" ? "✓" : $updateState === "error" ? "✗" : "○"}</span>
        <span class="check-detail">{$updateMsg}</span>
      </div>
    {/if}
  </div>

  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">System & Doctor Health Checks</h3>
        <p class="section-desc">Config, agents, and the window.</p>
      </div>
      <button class="sbtn" on:click={copyDiagnostics}>
        <Icon name="copy" size={12} />
        <span>Copy Diagnostics</span>
      </button>
    </div>

    <div class="checks-list">
      {#if diagTimedOut}
        <div class="check-row">
          <span class="check-icon fail">✗</span>
          <span class="check-name">diagnostics</span>
          <span class="check-detail">Timed out after 12s. Providers check themselves live under Agents.</span>
          <button class="sbtn" on:click={() => void load()}>Run again</button>
        </div>
      {/if}
      {#each quick ?? [] as c}
        <div class="check-row">
          <span class="check-icon {c.ok ? 'pass' : 'fail'}">{c.ok ? "✓" : "✗"}</span>
          <span class="check-name">{c.name}</span>
          <span class="check-detail">{c.detail}</span>
        </div>
      {/each}
    </div>

  </div>

  <div class="pref-section">
    <h3 class="section-title">Session Management</h3>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Stored Conversations</span>
        <span class="field-hint">{threadCount} total threads recorded under <code>~/.parzi/sessions</code></span>
      </div>
      <button class="sbtn danger" on:click={purgeSessions}>
        <Icon name="trash" size={12} />
        <span>Purge Finished Sessions</span>
      </button>
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Keyboard Shortcuts</h3>
    <div class="shortcuts-grid">
      {#each [
        ["Ctrl + T", "New tab"],
        ["Ctrl + W", "Close tab"],
        ["Ctrl + K", "Sessions and commands"],
        ["Ctrl + Tab", "Next tab"],
        ["Ctrl + ,", "Settings"],
        ["Enter", "Send"],
        ["Shift + Enter", "New line"],
        ["Esc", "Stop the run, or close what is open"],
      ] as [shortcut, action]}
        <div class="shortcut-item">
          <kbd>{shortcut}</kbd>
          <span class="shortcut-action">{action}</span>
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .checks-list { display: flex; flex-direction: column; gap: 4px; max-height: 260px; overflow-y: auto; }
  .check-row {
    display: flex; align-items: center; gap: 8px; padding: 6px 10px;
    background: var(--panel); border-radius: 6px; font-size: 12px;
  }
  .check-icon.pass { color: var(--ok); font-weight: 700; }
  .check-icon.fail { color: var(--bad); font-weight: 700; }
  .check-name { color: var(--text); font-weight: 500; min-width: 120px; flex: none; }
  .check-detail { color: var(--muted); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .upd-bar { height: 4px; border-radius: 2px; background: var(--line); overflow: hidden; margin-top: 8px; }
  .upd-fill { height: 100%; background: var(--accent); border-radius: 2px; transition: width 0.2s ease; }
  .shortcuts-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 6px 16px; }
  .shortcut-item {
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 0; border-bottom: 1px solid var(--line);
  }
  kbd {
    font-family: var(--mono), ui-monospace, monospace; font-size: 11px;
    background: var(--line); border: 1px solid var(--line);
    padding: 2px 6px; border-radius: 4px; color: var(--text);
  }
  .shortcut-action { font-size: 12px; color: var(--muted); }
</style>
