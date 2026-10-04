<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Check } from "../api";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  let quick: Check[] | null = null;

  let threadCount = 0;
  let err = "";

  let appVersion = "";
  let updateState: "idle" | "checking" | "available" | "uptodate" | "downloading" | "ready" | "error" = "idle";
  let updateMsg = "";
  let updateVersion = "";
  let dlTotal = 0;
  let dlDone = 0;
  let pendingUpdate: { version: string; body?: string; downloadAndInstall: (cb?: (e: unknown) => void) => Promise<void> } | null = null;

  async function checkUpdates() {
    updateState = "checking";
    updateMsg = "Checking for updates…";
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const u = await check();
      if (!u) {
        updateState = "uptodate";
        updateMsg = "You're on the latest version.";
        return;
      }
      pendingUpdate = u as unknown as typeof pendingUpdate;
      updateVersion = u.version;
      updateState = "available";
      updateMsg = `v${u.version} is available.`;
    } catch (e) {
      updateState = "error";
      updateMsg = `Update check failed (${e}). Dev builds check nothing — install from a release to update.`;
    }
  }

  async function installUpdate() {
    if (!pendingUpdate) return;
    updateState = "downloading";
    dlTotal = 0;
    dlDone = 0;
    updateMsg = "Downloading…";
    try {
      await pendingUpdate.downloadAndInstall((e: unknown) => {
        const ev = e as { event: string; data?: { contentLength?: number; chunkLength?: number } };
        if (ev.event === "Started") dlTotal = ev.data?.contentLength ?? 0;
        else if (ev.event === "Progress") dlDone += ev.data?.chunkLength ?? 0;
        else if (ev.event === "Finished") updateMsg = "Finishing install…";
      });
      updateState = "ready";
      updateMsg = "Update installed — restart Parzi to use it.";
      notify("Update installed — restart Parzi");
    } catch (e) {
      updateState = "error";
      updateMsg = `Install failed: ${e}`;
    }
  }

  onMount(async () => {
    try {
      const [q, threads, v] = await Promise.all([
        api.runDoctorQuick(),
        api.listThreads(),
        api.appVersion().catch(() => ""),
      ]);
      quick = q;
      threadCount = threads.length;
      appVersion = v;
    } catch (e) {
      err = String(e);
    }
  });

  function copyDiagnostics() {
    const all = [...(quick ?? [])];
    navigator.clipboard.writeText(all.map((c) => `${c.ok ? "PASS" : "FAIL"} [${c.name}] ${c.detail}`).join("\n"));
    notify("Diagnostics copied to clipboard");
  }

  async function purgeSessions() {
    try {
      const count = await api.purgeSessions();
      notify(`Purged ${count} completed sessions`);
      threadCount = (await api.listThreads()).length;
    } catch (e) {
      notify(`Purge failed: ${e}`);
    }
  }

</script>

{#if err}
  <div class="load-err"><span>{err}</span></div>
{:else if !quick}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
    <div class="skel" />
  </div>
{:else}
  <div class="pref-section" id="app-updates">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Updates</h3>
        <p class="section-desc">Parzi {appVersion ? `v${appVersion}` : ""} · stable channel · checks GitHub releases.</p>
      </div>
      {#if updateState === "available"}
        <button class="sbtn" on:click={installUpdate}>
          <span>Download & install v{updateVersion}</span>
        </button>
      {:else}
        <button class="sbtn" disabled={updateState === "checking" || updateState === "downloading"} on:click={checkUpdates}>
          <span>{updateState === "checking" ? "Checking…" : "Check for updates"}</span>
        </button>
      {/if}
    </div>
    {#if updateState === "downloading" && dlTotal > 0}
      <div class="upd-bar"><div class="upd-fill" style={`width:${Math.min(100, Math.round((dlDone / dlTotal) * 100))}%`} /></div>
    {/if}
    {#if updateMsg && updateState !== "idle"}
      <div class="check-row">
        <span class="check-icon {updateState === "ready" || updateState === "uptodate" ? "pass" : updateState === "error" ? "fail" : ""}">{updateState === "ready" || updateState === "uptodate" ? "✓" : updateState === "error" ? "✗" : "○"}</span>
        <span class="check-detail">{updateMsg}</span>
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
      {#each quick as c}
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
  #app-updates { scroll-margin-top: 8px; }
</style>
