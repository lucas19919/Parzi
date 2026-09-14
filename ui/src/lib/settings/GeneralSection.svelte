<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ParziConfig } from "../api";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  let cfg: ParziConfig | null = null;
  let err = "";

  onMount(async () => {
    try {
      cfg = await api.getConfig();
    } catch (e) {
      err = String(e);
    }
  });

  async function saveCfg() {
    if (!cfg) return;
    try {
      await api.saveConfig(cfg);
    } catch (e) {
      notify(`Save failed: ${e}`);
      try {
        cfg = await api.getConfig();
      } catch {}
    }
  }

  function toggleShell() {
    if (!cfg) return;
    const tools = cfg.lanes.default_allowed_tools || [];
    cfg.lanes.default_allowed_tools = tools.includes("shell.exec")
      ? tools.filter((t) => t !== "shell.exec")
      : [...tools, "shell.exec"];
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
    <h3 class="section-title">Agent Autonomy Policy</h3>
    <p class="section-desc">Controls how terminal commands and local tools require approval before running.</p>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Tool Execution Policy</span>
        <span class="field-hint">
          {cfg.lanes.default_mode === "ask"
            ? "Ask: Prompts for confirmation on each tool execution"
            : cfg.lanes.default_mode === "auto"
            ? "Turbo: Automatically executes tool calls in project directory"
            : "Lockdown: Strictly blocks all local shell & mutating tool execution"}
        </span>
      </div>
      <SegControl
        options={[{ value: "ask", label: "Ask" }, { value: "auto", label: "Turbo" }, { value: "deny", label: "Lockdown" }]}
        value={cfg.lanes.default_mode}
        on:pick={(e) => { if (cfg) cfg.lanes.default_mode = e.detail; saveCfg(); }}
      />
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Shell Commands (shell.exec)</span>
        <span class="field-hint">Allows agent to run bash / powershell scripts in your repository</span>
      </div>
      <Switch
        on={cfg.lanes.default_allowed_tools?.includes("shell.exec") ?? false}
        title="Allow the agent to run shell commands"
        on:toggle={toggleShell}
      />
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Orchestration & Limits</h3>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Follow-up messages while busy</span>
        <span class="field-hint">Behavior when sending prompts while an agent is currently running</span>
      </div>
      <SegControl
        options={[{ value: "queue", label: "Queue" }, { value: "reject", label: "Reject" }]}
        value={cfg.orchestrator.queue_when_busy ? "queue" : "reject"}
        on:pick={(e) => { if (cfg) cfg.orchestrator.queue_when_busy = e.detail === "queue"; saveCfg(); }}
      />
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Maximum Parallel Agent Runs</span>
        <span class="field-hint">Concurrent background agents allowed: <b>{cfg.orchestrator.max_concurrent}</b></span>
      </div>
      <div class="slider-wrapper">
        <input type="range" min="1" max="8" bind:value={cfg.orchestrator.max_concurrent} on:change={saveCfg} />
        <span class="slider-val">{cfg.orchestrator.max_concurrent}</span>
      </div>
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Maximum Reasoning Steps Per Turn</span>
        <span class="field-hint">Prevents infinite agent run loops: <b>{cfg.lanes.max_steps} steps</b></span>
      </div>
      <div class="slider-wrapper">
        <input type="range" min="4" max="64" bind:value={cfg.lanes.max_steps} on:change={saveCfg} />
        <span class="slider-val">{cfg.lanes.max_steps}</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .slider-wrapper { display: flex; align-items: center; gap: 10px; min-width: 180px; }
  .slider-wrapper input[type="range"] { flex: 1; accent-color: var(--accent); }
  .slider-val {
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px; color: var(--text-3);
    width: 38px; text-align: right;
  }
</style>
