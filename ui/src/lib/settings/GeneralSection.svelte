<script lang="ts">
  import { onMount } from "svelte";
  import { api, browserPrefs, type BrowserPrefs } from "../api";
  import Icon from "../Icon.svelte";
  import { notifyLevel, notifySounds, type NotifyLevel } from "../notifyPrefs";
  import SegControl from "./SegControl.svelte";
  import { check as checkUpdates, devChannel, dlDone, dlTotal, installUpdate, setDevChannel, updateMsg, updateState, updateVersion } from "../updateStore";
  import { clearFinishedSessions } from "../sessions";
  import ConnectionsSection from "./ConnectionsSection.svelte";
  import Switch from "./Switch.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};
  export let onSetup: () => void = () => {};
  export let onReport: () => void = () => {};

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

  const setLevel = (v: string) => notifyLevel.set((v === "all" || v === "off" ? v : "needed") as NotifyLevel);
  const NOTIFY = [
    { value: "all", label: "Everything" },
    { value: "needed", label: "When needed" },
    { value: "off", label: "Nothing" },
  ];

  onMount(() => {
    browserPrefs.get().then((p) => (web = p)).catch(() => {});
    api.appVersion().then((v) => (version = v)).catch(() => {});
    void countSessions();
  });
</script>

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Updates</span></div>
    <div class="row">
      <span class="row-label">Parzi {version}
        {#if $updateState === "error" || $updateState === "uptodate" || $updateState === "ready"}
          <small class:bad={$updateState === "error"}>{$updateState === "error" ? "Couldn't check for updates." : $updateMsg}</small>
        {/if}
      </span>
      <span class="row-ctl auto">
        {#if $updateState === "ready"}
          <button class="btn" on:click={installUpdate}>Install {$updateVersion}</button>
        {:else if $updateState === "available" || $updateState === "downloading"}
          <span class="progress-text">Downloading {$updateVersion}{pct ? ` · ${pct}%` : "…"}</span>
        {:else}
          <button class="btn" disabled={$updateState === "checking"} on:click={() => checkUpdates(true)}>
            {$updateState === "checking" ? "Checking…" : "Check for updates"}
          </button>
        {/if}
      </span>
    </div>
    <div class="row">
      <span class="row-label">Dev builds</span>
      <span class="row-ctl auto"><Switch on={$devChannel} title="Dev builds" on:toggle={() => setDevChannel(!$devChannel)} /></span>
    </div>
  </div>
</div>

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Notifications</span></div>
    <div class="row ctl-row">
      <span class="row-label">Tell me</span>
      <SegControl
        label="Notifications"
        options={NOTIFY}
        value={$notifyLevel}
        on:pick={(e) => setLevel(e.detail)}
      />
    </div>
    <div class="row">
      <span class="row-label">Sound when an agent needs me</span>
      <span class="row-ctl auto"><Switch on={$notifySounds} title="Sound when an agent needs me" on:toggle={() => notifySounds.set(!$notifySounds)} /></span>
    </div>
  </div>
</div>

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Browser</span></div>
    {#if web}
      <div class="row">
        <span class="row-label">Agent tabs use your sign-ins</span>
        <span class="row-ctl auto"><Switch on={web.agentSignIns} title="Agent tabs use your sign-ins" on:toggle={() => setWeb("agentSignIns")} /></span>
      </div>
      <div class="row">
        <span class="row-label">Developer tools for pages
          {#if !web.windows}<small>Windows only for now.</small>{/if}
        </span>
        <span class="row-ctl auto"><Switch on={web.devtools} title="Developer tools for pages" on:toggle={() => setWeb("devtools")} /></span>
      </div>
    {/if}
  </div>
</div>

<ConnectionsSection which="remote" {notify} on:setupRemote />

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Sessions</span></div>
    <div class="row">
      <span class="row-label">{sessions} {sessions === 1 ? "session" : "sessions"}</span>
      <span class="row-ctl auto"><button class="btn" on:click={clearSessions}><Icon name="trash" size={12} /><span>Delete</span></button></span>
    </div>
  </div>
</div>

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Set up Parzi</span></div>
    <div class="row">
      <span class="row-label">Guided setup<small>Agents, browser and the remote, step by step</small></span>
      <span class="row-ctl auto"><button class="btn" on:click={onSetup}><Icon name="wrench" size={12} /><span>Set up</span></button></span>
    </div>
  </div>
</div>

<div class="pref-section">
  <div class="rows">
    <div class="row row-title"><span>Report a problem</span></div>
    <div class="row">
      <span class="row-label">Something broken?<small>Open a GitHub issue with the details filled in</small></span>
      <span class="row-ctl auto"><button class="btn" on:click={onReport}><Icon name="alert" size={12} /><span>Report</span></button></span>
    </div>
  </div>
</div>

<style>
  .progress-text {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-md);
    font-variant-numeric: tabular-nums;
  }
</style>
