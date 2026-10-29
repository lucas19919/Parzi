<script lang="ts">
  import { onMount } from "svelte";
  import { adblock, api, browserPrefs, vault, type BrowserPrefs, type VaultState } from "../api";
  import Icon from "../Icon.svelte";
  import { permissionName } from "../pageRules";
  import { tooltip } from "../tooltip";
  import { check as checkUpdates, dlDone, dlTotal, installUpdate, updateMsg, updateState, updateVersion } from "../updateStore";
  import { clearFinishedSessions } from "../sessions";
  import ConnectionsSection from "./ConnectionsSection.svelte";
  import Switch from "./Switch.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  let blockAds = true;
  let pw: VaultState | null = null;
  let pwBusy = false;
  let web: BrowserPrefs | null = null;

  let version = "";
  let sessions = 0;

  $: pct = $dlTotal > 0 ? Math.min(100, Math.round(($dlDone / $dlTotal) * 100)) : 0;

  async function setWeb(name: "devtools" | "agentSignIns") {
    if (!web) return;
    try {
      web = await browserPrefs.set(name, !web[name]);
    } catch (e) {
      notify(`Could not save: ${e}`, true);
    }
  }

  async function forgetSite(origin: string, kind: string) {
    try {
      web = await browserPrefs.forget(origin, kind);
    } catch (e) {
      notify(`Could not save: ${e}`, true);
    }
  }

  async function installVault() {
    pwBusy = true;
    try {
      await vault.install();
      const recheck = () => {
        window.removeEventListener("focus", recheck);
        vault.state().then((s) => (pw = s)).catch(() => {});
      };
      window.addEventListener("focus", recheck);
    } catch (e) {
      pwBusy = false;
      notify(`Could not start the install: ${e}`, true);
    }
  }

  async function pwAction() {
    if (!pw || pwBusy) return;
    pwBusy = true;
    try {
      pw = pw.status === "unlocked" ? await vault.lock() : await vault.unlock();
    } catch (e) {
      notify(String(e) === "missing" ? "Bitwarden isn't installed" : String(e), true);
      pw = await vault.state().catch(() => pw);
    } finally {
      pwBusy = false;
    }
  }

  async function toggleAds() {
    try {
      blockAds = (await adblock.enable(!blockAds)).enabled;
    } catch (e) {
      notify(`Could not save: ${e}`, true);
    }
  }

  async function countSessions() {
    sessions = (await api.listThreads().catch(() => [])).length;
  }

  async function clearSessions() {
    try {
      await clearFinishedSessions();
      await countSessions();
    } catch (e) {
      notify(`Could not clear sessions: ${e}`, true);
    }
  }

  onMount(() => {
    adblock.state("").then((s) => (blockAds = s.enabled)).catch(() => {});
    vault.state().then((s) => (pw = s)).catch(() => {});
    browserPrefs.get().then((p) => (web = p)).catch(() => {});
    api.appVersion().then((v) => (version = v)).catch(() => {});
    void countSessions();
  });
</script>

<div class="pref-section">
  <h3 class="section-title">Browser</h3>
  <div class="field-card">
    <div class="field-info">
      <span class="field-label">Block ads and trackers</span>
      {#if web && !web.windows}<span class="field-hint">Windows only for now.</span>{/if}
    </div>
    <Switch on={blockAds} title="Block ads and trackers" on:toggle={toggleAds} />
  </div>
  <div class="field-card">
    <div class="field-info">
      <span class="field-label">Passwords from Bitwarden</span>
      <span class="field-hint">
        {#if !pw}
          Checking…
        {:else if pw.status === "missing"}
          Not installed
        {:else if pw.status === "unlocked"}
          Unlocked{pw.email ? ` as ${pw.email}` : ""}
        {:else if pw.status === "unauthenticated"}
          Not signed in
        {:else}
          Locked{pw.email ? ` · ${pw.email}` : ""}
        {/if}
      </span>
    </div>
    {#if pw?.status === "missing"}
      <button class="sbtn" disabled={pwBusy} on:click={installVault}>{pwBusy ? "Installing…" : "Install"}</button>
    {:else if pw}
      <button class="sbtn" disabled={pwBusy} on:click={pwAction}>
        {pwBusy ? "Waiting…" : pw.status === "unlocked" ? "Lock" : pw.status === "unauthenticated" ? "Sign in" : "Unlock"}
      </button>
    {/if}
  </div>
  {#if web}
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Agent tabs use your sign-ins</span>
      </div>
      <Switch on={web.agentSignIns} title="Agent tabs use your sign-ins" on:toggle={() => setWeb("agentSignIns")} />
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Developer tools for pages</span>
        {#if !web.windows}<span class="field-hint">Windows only for now.</span>{/if}
      </div>
      <Switch on={web.devtools} title="Developer tools for pages" on:toggle={() => setWeb("devtools")} />
    </div>
    {#if web.sites.length}
      <div class="field-card sites">
        <div class="field-info">
          <span class="field-label">Site permissions</span>
          <ul class="site-list">
            {#each web.sites as s (s.origin + s.kind)}
              <li class="site">
                <span class="site-origin">{s.origin.replace(/^https:\/\//, "")}</span>
                <span class="site-kind">{permissionName(s.kind)}</span>
                <span class="site-state" class:allow={s.allow}>{s.allow ? "Allowed" : "Blocked"}</span>
                <button class="forget" use:tooltip={"Forget this choice"} on:click={() => forgetSite(s.origin, s.kind)}>
                  <Icon name="close" size={12} />
                </button>
              </li>
            {/each}
          </ul>
        </div>
      </div>
    {/if}
  {/if}
</div>

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

<ConnectionsSection which="remote" {notify} on:setupRemote />

<div class="pref-section">
  <h3 class="section-title">Sessions</h3>
  <div class="field-card">
    <span class="field-label">{sessions} {sessions === 1 ? "session" : "sessions"}</span>
    <button class="sbtn" on:click={clearSessions}><Icon name="trash" size={12} /><span>Clear finished</span></button>
  </div>
</div>

<style>
  .field-card.sites {
    align-items: flex-start;
  }
  .site-list {
    display: flex;
    flex-direction: column;
    margin: calc(var(--space) * 2) 0 0;
    padding: 0;
    list-style: none;
  }
  .site {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-height: 32px;
    padding: 0 var(--space) 0 calc(var(--space) * 2);
    border-radius: var(--radius);
    font-size: var(--fs-md);
  }
  .site:hover {
    background: var(--hover);
  }
  .site-origin {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .site-kind {
    flex: none;
    color: var(--muted);
  }
  .site-state {
    flex: none;
    width: 64px;
    color: var(--muted);
  }
  .site-state.allow {
    color: var(--text);
  }
  .forget {
    flex: none;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .forget:hover {
    background: var(--selected);
    color: var(--text);
  }
  .field-hint.bad {
    color: var(--bad);
  }
  .progress-text {
    flex: none;
    font-size: var(--fs-md);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
