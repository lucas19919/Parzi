<script lang="ts">
  import { onMount } from "svelte";
  import { adblock, api, browserPrefs, vault, type BrowserPrefs, type ParziConfig, type VaultState } from "../api";
  import Icon from "../Icon.svelte";
  import { permissionName } from "../pageRules";
  import { tooltip } from "../tooltip";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import { updateConfig } from "../config";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  let cfg: ParziConfig | null = null;
  let err = "";
  let blockAds = true;
  let pw: VaultState | null = null;
  let pwBusy = false;
  let web: BrowserPrefs | null = null;

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

  // Bitwarden's CLI installs in a terminal; coming back to the window
  // checks whether it is there now.
  async function installVault() {
    pwBusy = true;
    try {
      await vault.install();
      const recheck = () => {
        window.removeEventListener("focus", recheck);
        vault.state().then((s) => (pw = s)).catch(() => {});
        pwBusy = false;
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

  onMount(async () => {
    adblock.state("").then((s) => (blockAds = s.enabled)).catch(() => {});
    vault.state().then((s) => (pw = s)).catch(() => {});
    browserPrefs.get().then((p) => (web = p)).catch(() => {});
    try {
      cfg = await api.getConfig();
    } catch (e) {
      err = String(e);
    }
  });

  async function update(change: (c: ParziConfig) => void): Promise<boolean> {
    try {
      cfg = await updateConfig(change);
      return true;
    } catch (e) {
      notify(`Could not save: ${e}`, true);
      try {
        cfg = await api.getConfig();
      } catch {}
      return false;
    }
  }

  // The composer's permission starts from this default; tell it.
  async function pickMode(mode: string) {
    if (!cfg) return;
    if (await update((c) => (c.lanes.default_mode = mode))) document.dispatchEvent(new CustomEvent("parzi:default-mode", { detail: mode }));
  }

  async function toggleAds() {
    try {
      blockAds = (await adblock.enable(!blockAds)).enabled;
    } catch (e) {
      notify(`Could not save: ${e}`, true);
    }
  }

  function toggleShell() {
    if (!cfg) return;
    const off = !cfg.lanes.no_shell;
    void update((c) => (c.lanes.no_shell = off));
  }
</script>

{#if err}
  <div class="load-err"><span>{err}</span><button class="sbtn" on:click={() => location.reload()}>Retry</button></div>
{:else if !cfg}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
    <div class="skel" />
  </div>
{:else}
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
      <div class="field-card sites">
        <div class="field-info">
          <span class="field-label">Site permissions</span>
          {#if !web.sites.length}<span class="field-hint">None yet</span>{/if}
          {#if web.sites.length}
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
          {/if}
        </div>
      </div>
    {/if}
  </div>

  <div class="pref-section">
    <h3 class="section-title">Permissions</h3>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">New sessions start on</span>
        {#if cfg.lanes.default_mode === "deny"}<span class="field-hint">Agents never run commands or change files.</span>{/if}
      </div>
      <SegControl
        label="New sessions start on"
        options={[{ value: "ask", label: "Ask" }, { value: "auto", label: "Auto" }, { value: "deny", label: "Locked" }]}
        value={cfg.lanes.default_mode === "auto" || cfg.lanes.default_mode === "deny" ? cfg.lanes.default_mode : "ask"}
        on:pick={(e) => void pickMode(e.detail)}
      />
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Agents run commands</span>
      </div>
      <Switch
        on={!cfg.lanes.no_shell}
        title="Allow the agent to run shell commands"
        on:toggle={toggleShell}
      />
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Limits</h3>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Agents at once</span>
      </div>
      <div class="slider-wrapper">
        <input type="range" min="1" max="8" aria-label="Agents at once" bind:value={cfg.orchestrator.max_concurrent} on:change={(e) => update((c) => (c.orchestrator.max_concurrent = Number(e.currentTarget.value)))} />
        <span class="slider-val">{cfg.orchestrator.max_concurrent}</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .field-card.sites {
    align-items: flex-start;
  }
  .site-list {
    display: flex;
    flex-direction: column;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .site {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 32px;
    padding: 0 4px 0 8px;
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
  .slider-wrapper { display: flex; align-items: center; gap: 10px; min-width: 180px; }
  .slider-wrapper input[type="range"] { flex: 1; accent-color: var(--accent); }
  .slider-val {
    font-size: var(--fs-xs); color: var(--muted); font-variant-numeric: tabular-nums;
    width: 38px; text-align: right;
  }
</style>
