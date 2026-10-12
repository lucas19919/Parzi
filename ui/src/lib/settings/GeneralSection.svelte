<script lang="ts">
  import { onMount } from "svelte";
  import { adblock, api, browserPrefs, vault, BITWARDEN_INSTALL, type BrowserPrefs, type ParziConfig, type VaultState } from "../api";
  import Icon from "../Icon.svelte";
  import { permissionName } from "../pageRules";
  import { tooltip } from "../tooltip";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

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
      notify(`Save failed: ${e}`);
    }
  }

  async function forgetSite(origin: string, kind: string) {
    try {
      web = await browserPrefs.forget(origin, kind);
    } catch (e) {
      notify(`Save failed: ${e}`);
    }
  }

  async function pwAction() {
    if (!pw || pwBusy) return;
    pwBusy = true;
    try {
      pw = pw.status === "unlocked" ? await vault.lock() : await vault.unlock();
    } catch (e) {
      notify(String(e) === "missing" ? `Install the Bitwarden CLI: ${BITWARDEN_INSTALL}` : String(e));
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

  async function saveCfg(): Promise<boolean> {
    if (!cfg) return false;
    try {
      await api.saveConfig(cfg);
      return true;
    } catch (e) {
      notify(`Save failed: ${e}`);
      try {
        cfg = await api.getConfig();
      } catch {}
      return false;
    }
  }

  // The composer's permission starts from this default; tell it.
  async function pickMode(mode: string) {
    if (!cfg) return;
    cfg.lanes.default_mode = mode;
    if (await saveCfg()) document.dispatchEvent(new CustomEvent("parzi:default-mode", { detail: mode }));
  }

  async function toggleAds() {
    try {
      blockAds = (await adblock.enable(!blockAds)).enabled;
    } catch (e) {
      notify(`Save failed: ${e}`);
    }
  }

  function toggleShell() {
    if (!cfg) return;
    cfg.lanes.no_shell = !cfg.lanes.no_shell;
    saveCfg();
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
        <span class="field-hint">
          Uses the EasyList and EasyPrivacy lists. Allow a single site from the shield in the page toolbar.{web && !web.windows
            ? " Works on Windows only for now."
            : ""}
        </span>
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
            Install the Bitwarden CLI with <code>{BITWARDEN_INSTALL}</code>, then restart Parzi.
          {:else if pw.status === "unlocked"}
            Unlocked{pw.email ? ` as ${pw.email}` : ""}. Use the key in the page toolbar to fill a login.
          {:else if pw.status === "unauthenticated"}
            Not signed in. You sign in in Bitwarden's own prompt; Parzi never sees your master password.
          {:else}
            Locked{pw.email ? ` (${pw.email})` : ""}. You unlock in Bitwarden's own prompt; Parzi never sees your master password.
          {/if}
        </span>
      </div>
      {#if pw && pw.status !== "missing"}
        <button class="sbtn" disabled={pwBusy} on:click={pwAction}>
          {pwBusy ? "Waiting…" : pw.status === "unlocked" ? "Lock" : pw.status === "unauthenticated" ? "Sign in" : "Unlock"}
        </button>
      {/if}
    </div>
    {#if web}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">Agent tabs use your sign-ins</span>
          <span class="field-hint">
            Off: pages an agent opens get their own browser profile, without your cookies and accounts. Applies to agent pages
            opened from now on.
          </span>
        </div>
        <Switch on={web.agentSignIns} title="Agent tabs use your sign-ins" on:toggle={() => setWeb("agentSignIns")} />
      </div>
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">Developer tools for pages</span>
          <span class="field-hint">{web.windows ? "F12 and Inspect in web page tabs." : "Needs Windows for now."}</span>
        </div>
        <Switch on={web.devtools} title="Developer tools for pages" on:toggle={() => setWeb("devtools")} />
      </div>
      <div class="field-card sites">
        <div class="field-info">
          <span class="field-label">Site permissions</span>
          <span class="field-hint">
            {web.sites.length
              ? "What you allowed or blocked when a page asked. Remove one to be asked again."
              : web.windows
                ? "Sites you allow or block when they ask for your camera, location and the like show up here."
                : "Pages ask with the system prompt on this computer."}
          </span>
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
    <h3 class="section-title">Agent autonomy</h3>
    <p class="section-desc">Controls how terminal commands and local tools require approval before running.</p>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Tool policy</span>
        <span class="field-hint">
          {cfg.lanes.default_mode === "ask"
            ? "Ask: asks before each tool call."
            : cfg.lanes.default_mode === "auto"
            ? "Turbo: runs tool calls in the session folder without asking."
            : "Lockdown: blocks shell commands and file changes."}
        </span>
      </div>
      <SegControl
        label="Tool policy"
        options={[{ value: "ask", label: "Ask" }, { value: "auto", label: "Turbo" }, { value: "deny", label: "Lockdown" }]}
        value={cfg.lanes.default_mode}
        on:pick={(e) => void pickMode(e.detail)}
      />
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Shell commands</span>
        <span class="field-hint">Lets Build agents run commands in the project folder. Off: no agent gets a shell.</span>
      </div>
      <Switch
        on={!cfg.lanes.no_shell}
        title="Allow the agent to run shell commands"
        on:toggle={toggleShell}
      />
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Orchestration and limits</h3>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Follow-up messages while busy</span>
        <span class="field-hint">Behavior when sending prompts while an agent is currently running</span>
      </div>
      <SegControl
        label="Follow-up messages while busy"
        options={[{ value: "queue", label: "Queue" }, { value: "reject", label: "Reject" }]}
        value={cfg.orchestrator.queue_when_busy ? "queue" : "reject"}
        on:pick={(e) => { if (cfg) cfg.orchestrator.queue_when_busy = e.detail === "queue"; saveCfg(); }}
      />
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Parallel agent runs</span>
        <span class="field-hint">Concurrent background agents allowed: <b>{cfg.orchestrator.max_concurrent}</b></span>
      </div>
      <div class="slider-wrapper">
        <input type="range" min="1" max="8" aria-label="Parallel agent runs" bind:value={cfg.orchestrator.max_concurrent} on:change={saveCfg} />
        <span class="slider-val">{cfg.orchestrator.max_concurrent}</span>
      </div>
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Reasoning steps per turn</span>
        <span class="field-hint">Prevents infinite agent run loops: <b>{cfg.lanes.max_steps} steps</b></span>
      </div>
      <div class="slider-wrapper">
        <input type="range" min="4" max="64" aria-label="Reasoning steps per turn" bind:value={cfg.lanes.max_steps} on:change={saveCfg} />
        <span class="slider-val">{cfg.lanes.max_steps}</span>
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
