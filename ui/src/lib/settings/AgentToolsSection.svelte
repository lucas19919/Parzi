<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ParziConfig, type BuiltinTool } from "../api";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  const GROUP_ORDER = ["Files", "Shell", "Teamwork", "Plans", "Knowledge", "Display"];
  const GROUP_BLURB: Record<string, string> = {
    Files: "Project files the agent can touch.",
    Shell: "Commands run in the repository.",
    Teamwork: "Sessions working with sessions. Always on — Ask mode still approves spawning and messaging.",
    Plans: "The living project plan. Always on.",
    Knowledge: "Durable project memory. Always on.",
    Display: "Cards, diagrams and artifacts rendered in threads. Always on.",
  };

  /** Tools the lane policy force-enables everywhere (mirrors the orchestrator).
      Shown locked-on so the UI never promises a switch it cannot keep. */
  const ALWAYS_ON = new Set([
    "session.spawn",
    "session.send_message",
    "session.read_session",
    "session.list_sessions",
    "plan.read",
    "plan.update",
    "lane.dispatch",
    "knowledge.read",
    "knowledge.record",
    "ui.show_markdown",
    "ui.show_widget",
    "ui.show_diagram",
  ]);

  const I = {
    plus: "M12 5v14M5 12h14",
    x: "M18 6 6 18M6 6l12 12",
  };

  let cfg: ParziConfig | null = null;
  let tools: BuiltinTool[] = [];
  let loading = true;
  let err = "";
  let customPattern = "";
  let customErr = "";

  onMount(async () => {
    try {
      cfg = await api.getConfig();
      tools = await api.listBuiltinTools();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  $: patterns = cfg?.lanes.default_allowed_tools ?? [];
  $: serverNames = Object.keys(cfg?.mcp?.servers ?? {}).sort();
  $: grouped = GROUP_ORDER.map((group) => ({
    group,
    items: tools.filter((t) => t.group === group),
  })).filter((g) => g.items.length);

  async function persist(msg?: string) {
    if (!cfg) return;
    try {
      await api.saveConfig(cfg);
      cfg = cfg;
      if (msg) notify(msg);
    } catch (e) {
      notify(`Save failed: ${e}`);
    }
  }

  function patternAllows(list: string[], name: string): boolean {
    return list.some(
      (p) => p === "*" || p === name || (p.endsWith(".*") && name.startsWith(p.slice(0, -1))),
    );
  }

  /** Broader pattern covering `name` (null when exact or uncovered). */
  function covering(list: string[], name: string): string | null {
    for (const p of list) {
      if (p === name) return null;
      if (p === "*" || (p.endsWith(".*") && name.startsWith(p.slice(0, -1)))) return p;
    }
    return null;
  }

  function toggleTool(name: string) {
    if (!cfg || ALWAYS_ON.has(name)) return;
    const list = cfg.lanes.default_allowed_tools ?? [];
    const off = list.includes(name);
    cfg.lanes.default_allowed_tools = off
      ? list.filter((t) => t !== name)
      : [...list, name];
    cfg = cfg;
    void persist(off ? `Blocked ${name}` : `Allowed ${name}`);
  }

  function toggleGlobalPattern(pattern: string) {
    if (!cfg) return;
    const list = cfg.lanes.default_allowed_tools ?? [];
    const off = list.includes(pattern);
    cfg.lanes.default_allowed_tools = off
      ? list.filter((t) => t !== pattern)
      : [...list, pattern];
    cfg = cfg;
    void persist(off ? `Blocked ${pattern}` : `Allowed ${pattern}`);
  }

  function addCustomPattern() {
    customErr = "";
    const p = customPattern.trim();
    if (!cfg || !p) return;
    if (!/^[a-zA-Z0-9_*.-]+$/.test(p)) {
      customErr = "Letters, numbers, ., -, _, * only — e.g. github.* or shell.exec.";
      return;
    }
    if ((cfg.lanes.default_allowed_tools ?? []).includes(p)) {
      customErr = "That pattern is already allowed.";
      return;
    }
    cfg.lanes.default_allowed_tools = [...(cfg.lanes.default_allowed_tools ?? []), p];
    customPattern = "";
    cfg = cfg;
    void persist(`Allowed ${p}`);
  }

  function setGlobalMode(mode: string) {
    if (!cfg) return;
    cfg.lanes.default_mode = mode;
    cfg = cfg;
    void persist(mode === "auto" ? "Turbo: tools run without asking" : mode === "deny" ? "Lockdown: tools blocked" : "Ask: tools need approval");
  }
</script>

{#if err}
  <div class="load-err"><span>{err}</span></div>
{:else if (loading || !cfg)}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
  </div>
{:else}
  <div class="pref-section">
    <div>
      <h3 class="section-title">Agent tools</h3>
      <p class="section-desc">Built-in capabilities — files, shell, teamwork, plans, knowledge, display. These are <b>not</b> connectors: they ship with Parzi and are managed here. Connector tools live under <b>Connectors</b>.</p>
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Tool execution policy</span>
        <span class="field-hint">
          {cfg.lanes.default_mode === "auto"
            ? "Turbo: tools run without asking"
            : cfg.lanes.default_mode === "deny"
            ? "Lockdown: tools are blocked (per-tool Auto can still punch through)"
            : "Ask: each tool run needs approval (per-tool Auto/Ask/Deny overrides)"}
        </span>
      </div>
      <SegControl
        options={[{ value: "ask", label: "Ask" }, { value: "auto", label: "Turbo" }, { value: "deny", label: "Lockdown" }]}
        value={cfg.lanes.default_mode}
        on:pick={(e) => setGlobalMode(e.detail)}
      />
    </div>
  </div>

  {#each grouped as g (g.group)}
    <div class="pref-section">
      <div>
        <h3 class="section-title">{g.group}</h3>
        <p class="section-desc">{GROUP_BLURB[g.group] ?? ""}</p>
      </div>
      {#each g.items as t (t.name)}
        {@const locked = ALWAYS_ON.has(t.name)}
        {@const on = locked || patternAllows(patterns, t.name)}
        {@const via = !locked && on ? covering(patterns, t.name) : null}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label mono">{t.name}</span>
            <span class="field-hint">{t.blurb}{locked ? " · always on" : via ? ` · on via ${via}` : ""}</span>
          </div>
          <Switch
            on={on}
            disabled={locked}
            title={locked ? `${t.name} is always available` : on ? `Block ${t.name}` : `Allow ${t.name}`}
            on:toggle={() => toggleTool(t.name)}
          />
        </div>
      {/each}
    </div>
  {/each}

  {#if serverNames.length}
    <div class="pref-section">
      <div class="field-card col">
        <div class="field-info">
          <span class="field-label">Connector access</span>
          <span class="field-hint"><span class="mono">server.*</span> lets the agent use that connector's exposed tools. Turn one off to cut the whole connector without uninstalling it.</span>
        </div>
        <div class="perm-rows">
          {#each serverNames as name (name)}
            <div class="perm-row">
              <span class="mono perm-name">{name}.*</span>
              <Switch on={patternAllows(patterns, `${name}.`)} title={patternAllows(patterns, `${name}.`) ? `Block all ${name} tools` : `Allow ${name} tools`} on:toggle={() => toggleGlobalPattern(`${name}.*`)} />
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}

  <div class="pref-section">
    <div class="field-card col">
      <div class="field-info">
        <span class="field-label">Custom patterns</span>
        <span class="field-hint">Exact names (<span class="mono">github.create_issue</span>), prefixes (<span class="mono">github.*</span>) or <span class="mono">*</span>.</span>
      </div>
      {#if patterns.length}
        <div class="chip-row">
          {#each patterns as p (p)}
            <span class="chip mono">{p}<button class="chip-x" title={`Block ${p}`} on:click={() => toggleGlobalPattern(p)}><Icon d={I.x} size={10} /></button></span>
          {/each}
        </div>
      {/if}
      <div class="paste-row">
        <input class="txt mono" placeholder="e.g. github.create_issue  —  Enter to allow" bind:value={customPattern} on:keydown={(e) => e.key === "Enter" && addCustomPattern()} />
        <button class="sbtn" on:click={addCustomPattern} disabled={!customPattern.trim()}><Icon d={I.plus} size={12} /><span>Allow</span></button>
      </div>
      {#if customErr}
        <span class="form-err">{customErr}</span>
      {/if}
    </div>
  </div>
{/if}
