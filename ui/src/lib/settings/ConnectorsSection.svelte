<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ParziConfig, type Check, type McpServerTools } from "../api";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import Icon from "../Icon.svelte";
  import { MCP_PRESETS, PRESET_GROUPS, type McpPreset } from "./mcpPresets";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  interface McpServer {
    command: string;
    args: string[];
    env: Record<string, string>;
    allow: string[];
    deny: string[];
    tool_modes: Record<string, string>;
    timeout_ms: number;
    enabled: boolean;
  }

  const I = {
    plus: "M12 5v14M5 12h14",
    trash: "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
    pulse: "M22 12h-4l-3 9L9 3l-3 9H2",
    edit: "M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    chev: "m6 9 6 6 6-6",
    tools: "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z",
    shield: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z",
    x: "M18 6 6 18M6 6l12 12",
    book: "M4 19.5A2.5 2.5 0 0 1 6.5 17H20M4 19.5A2.5 2.5 0 0 0 6.5 22H20V2H6.5A2.5 2.5 0 0 0 4 4.5v15z",
  };

  const LOCAL_TOOLS = [
    { name: "fs.read", label: "Read files", hint: "Let the agent read files in the project" },
    { name: "fs.write", label: "Write files", hint: "Let the agent create / overwrite files" },
    { name: "fs.list", label: "List directories", hint: "Let the agent browse the project tree" },
    { name: "shell.exec", label: "Shell commands", hint: "Run bash / powershell in the repository" },
  ];

  let cfg: ParziConfig | null = null;
  let servers: Record<string, McpServer> = {};
  let loading = true;
  let err = "";
  let probes: Check[] | null = null;
  let probing = false;

  let newName = "";
  let newCmd = "";
  let newArgs = "";
  let formErr = "";
  let showManual = false;

  let paste = "";
  let pasteName = "";
  let pasteErr = "";
  let pasting = false;

  /** Per-server tool inventories, loaded lazily when a card expands. */
  let toolsCache: Record<string, { loading: boolean; ok: boolean; error: string; tools: McpServerTools["tools"] }> = {};
  let toolsOpen: Record<string, boolean> = {};

  /** Per-preset install inputs (arg + env), kept while configuring. */
  let presetInputs: Record<string, { open: boolean; arg: string; env: Record<string, string> }> = {};
  let installing: string | null = null;

  let customPattern = "";
  let customErr = "";

  function sanitizeName(s: string): string {
    return s.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 48);
  }

  /** Guess a friendly name from a command line: package → filesystem, fetch, sqlite… */
  function autoName(command: string, args: string[]): string {
    const toks = [command, ...args];
    let pkg = "";
    for (const raw of toks) {
      const t = raw.trim().replace(/^["']|["']$/g, "");
      if (!t || t.startsWith("-")) continue;
      if (["npx", "uvx", "node", "python", "python3", "cmd", "/c"].includes(t.toLowerCase())) continue;
      pkg = t;
      if (t.includes("/") || t.startsWith("@") || t.startsWith("mcp-")) break;
    }
    if (!pkg) return "server";
    let base = pkg.split(/[\\/]/).pop() ?? pkg;
    base = base.split("@")[0] || base;
    base = base.replace(/^(mcp-server-|mcp-|server-)/, "").replace(/-server$/, "");
    return sanitizeName(base) || "server";
  }

  function uniqueName(base: string, taken: Set<string>): string {
    let n = sanitizeName(base) || "server";
    if (!/^[a-z0-9]/i.test(n)) n = `server-${n}`;
    let out = n;
    let i = 2;
    while (taken.has(out)) out = `${n}-${i++}`;
    taken.add(out);
    return out;
  }

  function splitCmd(line: string): string[] {
    const m = line.match(/"([^"]*)"|'([^']*)'|\S+/g) ?? [];
    return m.map((t) => t.replace(/^["']|["']$/g, "").trim()).filter(Boolean);
  }

  interface PastedServer {
    name: string;
    command: string;
    args: string[];
    env: Record<string, string>;
  }

  /**
   * Paste-anything: Claude-style {"mcpServers": {...}} JSON, a single
   * {"command": ...} object, or a bare `npx …` / `uvx …` command line.
   */
  function parsePasted(taken: Set<string>): PastedServer[] {
    const t = paste.trim();
    if (!t) throw new Error("Paste something first.");
    if (t.startsWith("{")) {
      let obj: unknown;
      try {
        obj = JSON.parse(t);
      } catch {
        throw new Error("That JSON doesn't parse — copy the whole { … } block.");
      }
      if (typeof obj !== "object" || obj === null) throw new Error("Expected a JSON object.");
      const o = obj as Record<string, unknown>;
      const hasBag = o.mcpServers && typeof o.mcpServers === "object";
      const entries: [string, unknown][] = hasBag
        ? Object.entries(o.mcpServers as Record<string, unknown>)
        : [["", o]];
      const out: PastedServer[] = [];
      for (const [key, v] of entries) {
        if (typeof v !== "object" || v === null) throw new Error(`"${key || "server"}" isn't an object.`);
        const s = v as Record<string, unknown>;
        if (typeof s.command !== "string" || !s.command.trim()) {
          throw new Error(`"${key || "server"}" needs a "command".`);
        }
        const args = Array.isArray(s.args) ? s.args.map(String) : [];
        const env: Record<string, string> = {};
        if (s.env && typeof s.env === "object") {
          for (const [k, val] of Object.entries(s.env as Record<string, unknown>)) env[k] = String(val);
        }
        out.push({
          name: uniqueName(key || pasteName.trim() || autoName(s.command, args), taken),
          command: s.command.trim(),
          args,
          env,
        });
      }
      if (!out.length) throw new Error("No servers found in that JSON.");
      return out;
    }
    const toks = splitCmd(t.split("\n")[0]);
    if (!toks.length) throw new Error("Paste something first.");
    const [command, ...args] = toks;
    return [
      { name: uniqueName(pasteName.trim() || autoName(command, args), taken), command, args, env: {} },
    ];
  }

  async function addPasted() {
    pasteErr = "";
    if (pasting) return;
    pasting = true;
    try {
      const found = parsePasted(new Set(Object.keys(servers)));
      const next = { ...servers };
      for (const s of found) {
        next[s.name] = {
          command: s.command,
          args: s.args,
          env: s.env,
          allow: [],
          deny: [],
          tool_modes: {},
          timeout_ms: 30_000,
          enabled: true,
        };
      }
      servers = next;
      await persist(found.length === 1 ? `Added ${found[0].name}` : `Added ${found.length} connectors`);
      paste = "";
      pasteName = "";
      void probe();
    } catch (e) {
      pasteErr = e instanceof Error ? e.message : String(e);
    } finally {
      pasting = false;
    }
  }

  function presetInput(id: string): { open: boolean; arg: string; env: Record<string, string> } {
    return presetInputs[id] ?? { open: false, arg: "", env: {} };
  }

  function setPresetArg(id: string, e: Event) {
    const el = e.target as HTMLInputElement | null;
    presetInputs = { ...presetInputs, [id]: { ...presetInput(id), arg: el?.value ?? "" } };
  }

  function setPresetEnv(id: string, key: string, e: Event) {
    const el = e.target as HTMLInputElement | null;
    const cur = presetInput(id);
    presetInputs = { ...presetInputs, [id]: { ...cur, env: { ...cur.env, [key]: el?.value ?? "" } } };
  }

  async function installPreset(p: McpPreset) {
    if (installing) return;
    const inp = presetInput(p.id);
    // Validate required fields before touching config.
    if (p.argField?.required && !inp.arg.trim()) {
      notify(`${p.label}: ${p.argField.label} is required`);
      presetInputs = { ...presetInputs, [p.id]: { ...inp, open: true } };
      return;
    }
    for (const f of p.envFields) {
      if (f.required && !(inp.env[f.key] ?? "").trim()) {
        notify(`${p.label}: ${f.label} is required`);
        presetInputs = { ...presetInputs, [p.id]: { ...inp, open: true } };
        return;
      }
    }
    installing = p.id;
    try {
      const name = uniqueName(p.name, new Set(Object.keys(servers)));
      const args = [...p.args];
      if (inp.arg.trim()) args.push(inp.arg.trim());
      const env: Record<string, string> = {};
      for (const [k, v] of Object.entries(inp.env)) {
        if (v.trim()) env[k] = v.trim();
      }
      servers = {
        ...servers,
        [name]: { command: p.command, args, env, allow: [], deny: [], tool_modes: {}, timeout_ms: 30_000, enabled: true },
      };
      presetInputs = { ...presetInputs, [p.id]: { open: false, arg: "", env: {} } };
      await persist(`Added ${name}`);
      void probe();
    } finally {
      installing = null;
    }
  }

  let editing: string | null = null;
  let draft = { command: "", args: "", env: "", timeout: "30" };
  let draftErr = "";

  onMount(async () => {
    try {
      cfg = await api.getConfig();
      servers = normalize((cfg.mcp?.servers ?? {}) as Record<string, unknown>);
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  function strList(v: unknown): string[] {
    return Array.isArray(v) ? v.map(String).filter((s) => s.trim().length > 0) : [];
  }

  function strMap(v: unknown): Record<string, string> {
    const out: Record<string, string> = {};
    if (v && typeof v === "object") {
      for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
        const mode = String(val);
        if (["auto", "ask", "deny"].includes(mode)) out[k] = mode;
      }
    }
    return out;
  }

  function normalize(raw: Record<string, unknown>): Record<string, McpServer> {
    const out: Record<string, McpServer> = {};
    for (const [name, v] of Object.entries(raw)) {
      const s = (v ?? {}) as Partial<McpServer> & { tool_modes?: unknown };
      const env: Record<string, string> = {};
      if (s.env && typeof s.env === "object") {
        for (const [k, val] of Object.entries(s.env as Record<string, unknown>)) env[k] = String(val);
      }
      out[name] = {
        command: typeof s.command === "string" ? s.command : "",
        args: Array.isArray(s.args) ? s.args.map(String) : [],
        env,
        allow: strList(s.allow),
        deny: strList((s as Partial<McpServer>).deny),
        tool_modes: strMap(s.tool_modes),
        timeout_ms: typeof s.timeout_ms === "number" ? s.timeout_ms : 30_000,
        enabled: s.enabled !== false,
      };
    }
    return out;
  }

  $: serverNames = Object.keys(servers).sort();
  $: globalPatterns = cfg?.lanes.default_allowed_tools ?? [];

  async function persist(msg?: string) {
    if (!cfg) return;
    try {
      cfg.mcp = { ...(cfg.mcp ?? {}), servers };
      await api.saveConfig(cfg);
      cfg = cfg;
      servers = { ...servers };
      if (msg) notify(msg);
    } catch (e) {
      notify(`Save failed: ${e}`);
    }
  }

  /* ---------- general permissions (layer 1: lane allowlist) ---------- */

  function patternAllows(patterns: string[], name: string): boolean {
    return patterns.some(
      (p) => p === "*" || p === name || (p.endsWith(".*") && name.startsWith(p.slice(0, -1))),
    );
  }

  function toggleGlobalPattern(pattern: string) {
    if (!cfg) return;
    const list = cfg.lanes.default_allowed_tools ?? [];
    cfg.lanes.default_allowed_tools = list.includes(pattern)
      ? list.filter((t) => t !== pattern)
      : [...list, pattern];
    cfg = cfg;
    void persist(list.includes(pattern) ? `Blocked ${pattern}` : `Allowed ${pattern}`);
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

  /* ---------- installed servers ---------- */

  function toggleEnabled(name: string) {
    const s = servers[name];
    if (!s) return;
    servers[name] = { ...s, enabled: !s.enabled };
    void persist(s.enabled ? `${name} disabled` : `${name} enabled`);
  }

  function removeServer(name: string) {
    const next = { ...servers };
    delete next[name];
    servers = next;
    const tc = { ...toolsCache };
    delete tc[name];
    toolsCache = tc;
    const to = { ...toolsOpen };
    delete to[name];
    toolsOpen = to;
    void persist(`Removed ${name}`);
  }

  function addServer() {
    formErr = "";
    const name = newName.trim();
    const cmd = newCmd.trim();
    if (!/^[a-z0-9][a-z0-9_-]*$/i.test(name)) {
      formErr = "Name: letters, numbers, dashes.";
      return;
    }
    if (!cmd) {
      formErr = "Command is required.";
      return;
    }
    if (servers[name]) {
      formErr = "A server with that name already exists.";
      return;
    }
    servers = {
      ...servers,
      [name]: {
        command: cmd,
        args: newArgs.split(/\s+/).map((a) => a.trim()).filter(Boolean),
        env: {},
        allow: [],
        deny: [],
        tool_modes: {},
        timeout_ms: 30_000,
        enabled: true,
      },
    };
    newName = "";
    newCmd = "";
    newArgs = "";
    void persist(`Added ${name}`);
  }

  async function probe() {
    probing = true;
    try {
      probes = await api.runDoctorMcp();
    } catch (e) {
      probes = [{ name: "mcp", ok: false, detail: String(e) }];
    } finally {
      probing = false;
    }
  }

  function probeFor(name: string): Check | null {
    return probes?.find((c) => c.name === `mcp:${name}`) ?? null;
  }

  /* ---------- per-server tool browser (layer 2: exposure + policy) ---------- */

  async function loadTools(name: string) {
    if (toolsCache[name]?.loading) return;
    toolsCache = { ...toolsCache, [name]: { loading: true, ok: false, error: "", tools: toolsCache[name]?.tools ?? [] } };
    try {
      const res = await api.listMcpTools(name);
      toolsCache = { ...toolsCache, [name]: { loading: false, ok: res.ok, error: res.error ?? "", tools: res.tools } };
    } catch (e) {
      toolsCache = { ...toolsCache, [name]: { loading: false, ok: false, error: String(e), tools: [] } };
    }
  }

  function toggleTools(name: string) {
    const open = !toolsOpen[name];
    toolsOpen = { ...toolsOpen, [name]: open };
    if (open && !toolsCache[name] && servers[name]?.enabled) void loadTools(name);
  }

  /** Exposure toggle: deny-list mode while `allow` is empty, allow-list mode otherwise. */
  function setToolExposed(serverName: string, tool: string, on: boolean) {
    const s = servers[serverName];
    if (!s) return;
    let allow = [...s.allow];
    let deny = [...s.deny];
    if (on) {
      deny = deny.filter((t) => t !== tool);
      if (allow.length && !allow.includes(tool)) allow.push(tool);
    } else if (allow.length) {
      allow = allow.filter((t) => t !== tool);
    } else if (!deny.includes(tool)) {
      deny.push(tool);
    }
    servers = { ...servers, [serverName]: { ...s, allow, deny } };
    syncCacheTool(serverName, tool, { exposed: on });
    void persist(on ? `${serverName}.${tool} exposed` : `${serverName}.${tool} hidden`);
  }

  function resetExposure(serverName: string) {
    const s = servers[serverName];
    if (!s) return;
    servers = { ...servers, [serverName]: { ...s, allow: [], deny: [] } };
    const c = toolsCache[serverName];
    if (c) {
      toolsCache = {
        ...toolsCache,
        [serverName]: { ...c, tools: c.tools.map((t) => ({ ...t, exposed: true })) },
      };
    }
    void persist(`${serverName}: all tools exposed`);
  }

  function setToolMode(serverName: string, tool: string, mode: string) {
    const s = servers[serverName];
    if (!s) return;
    const modes = { ...s.tool_modes };
    if (mode === "inherit") delete modes[tool];
    else modes[tool] = mode;
    servers = { ...servers, [serverName]: { ...s, tool_modes: modes } };
    syncCacheTool(serverName, tool, { mode: mode === "inherit" ? null : mode });
    const label = mode === "inherit" ? "follows general policy" : mode === "auto" ? "auto-runs" : mode === "deny" ? "blocked" : "always asks";
    void persist(`${serverName}.${tool} ${label}`);
  }

  function syncCacheTool(serverName: string, tool: string, patch: Partial<{ exposed: boolean; mode: string | null }>) {
    const c = toolsCache[serverName];
    if (!c) return;
    toolsCache = {
      ...toolsCache,
      [serverName]: { ...c, tools: c.tools.map((t) => (t.name === tool ? { ...t, ...patch } : t)) },
    };
  }

  function exposureSummary(s: McpServer): string {
    if (s.deny.length && !s.allow.length) return `${s.deny.length} hidden`;
    if (s.allow.length) return `${s.allow.length} pinned`;
    return "all exposed";
  }

  /* ---------- server editor ---------- */

  function startEdit(name: string) {
    const s = servers[name];
    if (!s) return;
    editing = name;
    draftErr = "";
    draft = {
      command: s.command,
      args: s.args.join(" "),
      env: Object.entries(s.env).map(([k, v]) => `${k}=${v}`).join("\n"),
      timeout: String(Math.max(1, Math.round(s.timeout_ms / 1000))),
    };
  }

  function saveEdit() {
    if (!editing || !servers[editing]) return;
    const cmd = draft.command.trim();
    if (!cmd) {
      draftErr = "Command is required.";
      return;
    }
    const secs = Number(draft.timeout);
    if (!Number.isFinite(secs) || secs < 1 || secs > 600) {
      draftErr = "Timeout: 1–600 seconds.";
      return;
    }
    const env: Record<string, string> = {};
    for (const line of draft.env.split("\n")) {
      const t = line.trim();
      if (!t) continue;
      const i = t.indexOf("=");
      if (i <= 0) {
        draftErr = `Bad env line (need KEY=value): ${t}`;
        return;
      }
      env[t.slice(0, i).trim()] = t.slice(i + 1).trim();
    }
    servers[editing] = {
      ...servers[editing],
      command: cmd,
      args: draft.args.split(/\s+/).map((a) => a.trim()).filter(Boolean),
      env,
      timeout_ms: Math.round(secs * 1000),
    };
    const done = editing;
    editing = null;
    void persist(`Saved ${done}`);
  }
</script>

{#if err}
  <div class="load-err"><span>{err}</span></div>
{:else if (loading)}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
  </div>
{:else}
  <!-- General permissions: lane allowlist + approval policy -->
  <div class="pref-section">
    <div>
      <h3 class="section-title">General permissions</h3>
      <p class="section-desc">What the agent may use at all. A tool reaches the agent only when <b>both</b> layers agree: allowed here <b>and</b> exposed on its connector card below.</p>
    </div>

    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Tool execution policy</span>
        <span class="field-hint">
          {cfg && cfg.lanes.default_mode === "auto"
            ? "Turbo: tools run without asking"
            : cfg && cfg.lanes.default_mode === "deny"
            ? "Lockdown: tools are blocked (per-tool Auto can still punch through)"
            : "Ask: each tool run needs approval (per-tool Auto/Ask/Deny overrides)"}
        </span>
      </div>
      {#if cfg}
        <SegControl
          options={[{ value: "ask", label: "Ask" }, { value: "auto", label: "Turbo" }, { value: "deny", label: "Lockdown" }]}
          value={cfg.lanes.default_mode}
          on:pick={(e) => setGlobalMode(e.detail)}
        />
      {/if}
    </div>

    {#each LOCAL_TOOLS as t (t.name)}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label mono">{t.name}</span>
          <span class="field-hint">{t.hint}</span>
        </div>
        <Switch on={patternAllows(globalPatterns, t.name)} title={patternAllows(globalPatterns, t.name) ? `Block ${t.name}` : `Allow ${t.name}`} on:toggle={() => toggleGlobalPattern(t.name)} />
      </div>
    {/each}

    {#if serverNames.length}
      <div class="field-card col">
        <div class="field-info">
          <span class="field-label">Connector access</span>
          <span class="field-hint"><span class="mono">server.*</span> lets the agent use that connector's exposed tools. Turn one off to cut the whole connector without uninstalling it.</span>
        </div>
        <div class="perm-rows">
          {#each serverNames as name (name)}
            <div class="perm-row">
              <span class="mono perm-name">{name}.*</span>
              <Switch on={patternAllows(globalPatterns, `${name}.`)} title={patternAllows(globalPatterns, `${name}.`) ? `Block all ${name} tools` : `Allow ${name} tools`} on:toggle={() => toggleGlobalPattern(`${name}.*`)} />
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <div class="field-card col">
      <div class="field-info">
        <span class="field-label">Custom patterns</span>
        <span class="field-hint">Exact names (<span class="mono">github.create_issue</span>), prefixes (<span class="mono">github.*</span>) or <span class="mono">*</span>. UI and teamwork tools stay on — rendering isn't execution.</span>
      </div>
      {#if globalPatterns.length}
        <div class="chip-row">
          {#each globalPatterns as p (p)}
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

  <!-- Installed connectors with tool browsers -->
  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Installed connectors</h3>
        <p class="section-desc">MCP servers Parzi can spawn. Stored in config under <b>mcp.servers</b> — saves apply instantly, no restart.</p>
      </div>
      <button class="sbtn" on:click={probe} disabled={probing}>
        <Icon d={I.pulse} size={12} />
        <span>{probing ? "Probing…" : "Probe servers"}</span>
      </button>
    </div>

    {#if !serverNames.length}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">No servers configured</span>
          <span class="field-hint">Install one below — or paste any MCP config.</span>
        </div>
      </div>
    {/if}

    {#each serverNames as name (name)}
      {@const s = servers[name]}
      {@const probe = probeFor(name)}
      {@const tc = toolsCache[name]}
      {@const open = !!toolsOpen[name]}
      <div class="field-card col">
        <div class="card-top">
          <div class="field-info">
            <span class="field-label">{name}
              {#if probe}
                <span class="status-badge {probe.ok ? 'ok' : 'expired'}">{probe.ok ? probe.detail : "unreachable"}</span>
              {:else}
                <span class="status-badge {s.enabled ? 'ok' : 'missing'}">{s.enabled ? "enabled" : "off"}</span>
              {/if}
              {#if tc && !tc.loading && tc.ok}
                <span class="status-badge ok">{tc.tools.length} tools</span>
              {/if}
            </span>
            <span class="field-hint mono">{s.command}{#if s.args.length} {s.args.join(" ")}{/if} · {Math.round(s.timeout_ms / 1000)}s{#if Object.keys(s.env).length} · {Object.keys(s.env).length} env{/if} · {exposureSummary(s)}</span>
            {#if probe && !probe.ok}
              <span class="field-hint">{probe.detail}</span>
            {/if}
          </div>
          <div class="row-actions">
            <Switch on={s.enabled} title={s.enabled ? `Disable ${name}` : `Enable ${name}`} on:toggle={() => toggleEnabled(name)} />
            <button class="sbtn" title={open ? `Hide ${name} tools` : `Show ${name} tools`} on:click={() => toggleTools(name)} disabled={!s.enabled}>
              <Icon d={I.tools} size={12} />
              <span>{open ? "Tools" : tc && tc.tools.length ? `${tc.tools.length} tools` : "Tools"}</span>
              <span class="chev {open ? 'up' : ''}"><Icon d={I.chev} size={12} /></span>
            </button>
            <button class="mini-btn" title={editing === name ? "Close editor" : `Edit ${name}`} on:click={() => (editing === name ? (editing = null) : startEdit(name))}>
              <Icon d={I.edit} size={12} />
            </button>
            <button class="sbtn danger" title={`Remove ${name}`} on:click={() => { if (editing === name) editing = null; removeServer(name); }}>
              <Icon d={I.trash} size={12} />
            </button>
          </div>
        </div>

        {#if open}
          <div class="tools-pane">
            {#if !s.enabled}
              <span class="field-hint">Enable {name} to browse its tools.</span>
            {:else if !tc || (tc.loading && !tc.tools.length)}
              <span class="field-hint">Starting {name}…</span>
            {:else if !tc.ok && !tc.tools.length}
              <span class="form-err">{tc.error || "Couldn't list tools."}</span>
              <div><button class="sbtn" on:click={() => loadTools(name)}><span>Retry</span></button></div>
            {:else}
              {#if tc.error}
                <span class="field-hint">{tc.error} — showing last known list.</span>
              {/if}
              <div class="tool-head">
                <span class="field-hint">{tc.tools.filter((t) => t.exposed).length}/{tc.tools.length} exposed · policy Inherit follows the general policy above</span>
                <div class="tool-head-actions">
                  {#if s.allow.length || s.deny.length}
                    <button class="link-btn" on:click={() => resetExposure(name)}>Expose all</button>
                  {/if}
                  <button class="link-btn" on:click={() => loadTools(name)} disabled={tc.loading}>{tc.loading ? "Refreshing…" : "Refresh"}</button>
                </div>
              </div>
              {#each tc.tools as t (t.name)}
                {@const blocked = !patternAllows(globalPatterns, t.qualified)}
                <div class="tool-row">
                  <Switch on={t.exposed} title={t.exposed ? `Hide ${t.qualified}` : `Expose ${t.qualified}`} on:toggle={() => setToolExposed(name, t.name, !t.exposed)} />
                  <div class="field-info">
                    <span class="field-label mono">{t.name}
                      {#if blocked && t.exposed}
                        <span class="status-badge missing" title="Exposed here, but the general permissions above block it">blocked above</span>
                      {:else if !t.exposed}
                        <span class="status-badge missing">hidden</span>
                      {/if}
                    </span>
                    {#if t.description}
                      <span class="field-hint">{t.description}</span>
                    {/if}
                    <span class="field-hint mono">{t.qualified}</span>
                  </div>
                  <SegControl
                    options={[{ value: "inherit", label: "Inherit" }, { value: "auto", label: "Auto" }, { value: "ask", label: "Ask" }, { value: "deny", label: "Deny" }]}
                    value={t.mode ?? "inherit"}
                    on:pick={(e) => setToolMode(name, t.name, e.detail)}
                  />
                </div>
              {/each}
            {/if}
          </div>
        {/if}

        {#if editing === name}
          <div class="form-grid">
            <label class="fld"><span>Command</span><input class="txt mono" bind:value={draft.command} /></label>
            <label class="fld"><span>Args (space-separated)</span><input class="txt mono" bind:value={draft.args} /></label>
            <label class="fld"><span>Env (one KEY=value per line)</span><textarea class="txt mono" rows="3" bind:value={draft.env} /></label>
            <label class="fld"><span>Timeout (seconds)</span><input class="txt mono" bind:value={draft.timeout} inputmode="numeric" /></label>
            <span class="field-hint">Tool exposure is managed in the Tools list above ({exposureSummary(s)}).</span>
            {#if draftErr}
              <span class="form-err">{draftErr}</span>
            {/if}
            <div class="btn-row">
              <button class="sbtn primary" on:click={saveEdit}><span>Save {name}</span></button>
              <button class="sbtn" on:click={() => (editing = null)}><span>Cancel</span></button>
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <!-- One-click standard connectors -->
  <div class="pref-section">
    <h3 class="section-title">Standard connectors</h3>
    <p class="section-desc">One-click installs. Keys stay in your local config — never logged. Community picks are labelled.</p>
    {#each PRESET_GROUPS as g (g)}
      {@const presets = MCP_PRESETS.filter((p) => p.group === g && !servers[p.name])}
      {#if presets.length}
        <span class="group-label">{g}</span>
        <div class="preset-grid">
          {#each presets as p (p.id)}
            {@const inp = presetInput(p.id)}
            <div class="preset col">
              <div class="preset-top">
                <div class="field-info">
                  <span class="field-label">{p.label}
                    {#if p.official}
                      <span class="status-badge ok">official</span>
                    {:else}
                      <span class="status-badge missing">community</span>
                    {/if}
                  </span>
                  <span class="field-hint">{p.desc}</span>
                  <span class="field-hint mono">{p.command} {p.args.join(" ")}</span>
                </div>
              </div>
              {#if p.argField || p.envFields.length}
                {#if inp.open}
                  <div class="form-grid">
                    {#if p.argField}
                      <label class="fld"><span>{p.argField.label}{p.argField.required ? " (required)" : ""} — {p.argField.hint}</span>
                        <input class="txt mono" placeholder={p.argField.placeholder} value={inp.arg} on:input={(e) => setPresetArg(p.id, e)} />
                      </label>
                    {/if}
                    {#each p.envFields as f (f.key)}
                      <label class="fld"><span>{f.label}{f.required ? " (required)" : " (optional)"}</span>
                        <input class="txt mono" type={f.secret ? "password" : "text"} placeholder={f.placeholder}
                          value={inp.env[f.key] ?? ""}
                          on:input={(e) => setPresetEnv(p.id, f.key, e)} />
                      </label>
                    {/each}
                  </div>
                {/if}
              {/if}
              <div class="preset-actions">
                <button class="link-btn" on:click={() => api.openExternalUrl(p.docs)} title="Open setup docs"><span class="link-inline"><Icon d={I.book} size={11} /> docs</span></button>
                {#if p.argField || p.envFields.length}
                  {#if !inp.open}
                    <button class="sbtn" on:click={() => (presetInputs = { ...presetInputs, [p.id]: { ...inp, open: true } })}><span>Configure</span></button>
                  {:else}
                    <button class="sbtn primary" on:click={() => installPreset(p)} disabled={installing === p.id}><span>{installing === p.id ? "Installing…" : "Install"}</span></button>
                  {/if}
                {:else}
                  <button class="sbtn" on:click={() => installPreset(p)} disabled={installing === p.id}><span>{installing === p.id ? "Installing…" : "Install"}</span></button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/each}
    {#if MCP_PRESETS.every((p) => servers[p.name])}
      <div class="field-card"><div class="field-info"><span class="field-label">All standards installed</span><span class="field-hint">Manage them in Installed connectors above.</span></div></div>
    {/if}
  </div>

  <!-- Custom: paste anything or manual -->
  <div class="pref-section">
    <h3 class="section-title">Custom connector</h3>
    <p class="section-desc">Paste the config from any MCP's README — or a bare <span class="mono">npx …</span> line. Name and settings are detected automatically.</p>
    <div class="field-card col">
      <textarea
        class="txt mono"
        rows="4"
        placeholder={'{"mcpServers": {"filesystem": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"]}}}   — or just:   npx -y @modelcontextprotocol/server-fetch'}
        bind:value={paste}
      />
      <div class="paste-row">
        <input class="txt" placeholder="Name (optional — auto-detected)" bind:value={pasteName} on:keydown={(e) => e.key === "Enter" && addPasted()} />
        <button class="sbtn primary" on:click={addPasted} disabled={pasting || !paste.trim()}>
          <Icon d={I.plus} size={12} />
          <span>{pasting ? "Adding…" : "Add"}</span>
        </button>
      </div>
      {#if pasteErr}
        <span class="form-err">{pasteErr}</span>
      {/if}
    </div>
    <button class="link-btn" on:click={() => (showManual = !showManual)}>
      {showManual ? "Hide manual setup ▴" : "Manual setup ▾"}
    </button>
    {#if showManual}
    <div class="field-card col">
      <div class="form-grid">
        <input class="txt" placeholder="name — e.g. filesystem" bind:value={newName} />
        <input class="txt mono" placeholder="command — e.g. npx" bind:value={newCmd} />
        <input
          class="txt mono"
          placeholder="args — e.g. -y @modelcontextprotocol/server-filesystem /tmp"
          bind:value={newArgs}
          on:keydown={(e) => e.key === "Enter" && addServer()}
        />
      </div>
      {#if formErr}
        <span class="form-err">{formErr}</span>
      {/if}
      <div>
        <button class="sbtn primary" on:click={addServer} disabled={!cfg}>
          <Icon d={I.plus} size={12} />
          <span>Add server</span>
        </button>
      </div>
    </div>
    {/if}
  </div>

  <div class="pref-section">
    <div class="field-card">
      <div class="field-info">
        <span class="field-label"><span class="perm-ico"><Icon d={I.shield} size={13} /></span> How permissions combine</span>
        <span class="field-hint">General permissions decide what the agent may touch. Each connector exposes its tools (all by default). Per-tool Auto / Ask / Deny overrides the general policy: Auto runs silently, Ask always prompts, Deny blocks even in Turbo.</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .mono { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11.5px; }
  .row-actions { display: inline-flex; align-items: center; gap: 8px; flex: none; flex-wrap: wrap; justify-content: flex-end; }
  .field-card.col { flex-direction: column; align-items: stretch; }
  .card-top { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .form-grid { display: flex; flex-direction: column; gap: 8px; }
  .fld { display: flex; flex-direction: column; gap: 4px; }
  .fld > span { font-size: 11px; color: var(--text-3); }
  .txt {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; padding: 7px 10px; width: 100%;
  }
  textarea.txt { resize: vertical; min-height: 56px; line-height: 1.5; }
  .txt::placeholder { color: var(--text-4); }
  .paste-row { display: grid; grid-template-columns: 1fr auto; gap: 8px; }
  .group-label { font-size: 11px; font-weight: 600; color: var(--text-3); text-transform: uppercase; letter-spacing: 0.6px; margin-top: 4px; }
  .preset-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .preset {
    display: flex; gap: 10px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-3); padding: 10px 14px;
  }
  .preset.col { flex-direction: column; align-items: stretch; }
  .preset-top { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .preset-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .link-inline { display: inline-flex; align-items: center; gap: 4px; }
  .link-btn {
    background: transparent; border: none; color: var(--text-3); font: inherit;
    font-size: 12px; cursor: pointer; padding: 2px 0; text-align: left; width: fit-content;
  }
  .link-btn:hover { color: var(--text); }
  .btn-row { display: flex; gap: 8px; }
  .mini-btn {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 28px; height: 28px; padding: 0 4px;
    background: transparent; border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text-3); cursor: pointer;
  }
  .mini-btn:hover { background: var(--surface-2); color: var(--text); }
  .form-err { font-size: 12px; color: var(--bad); }
  .status-badge { margin-left: 8px; }
  .chev { display: inline-flex; transition: transform 150ms ease; }
  .chev.up { transform: rotate(180deg); }
  .perm-rows { display: flex; flex-direction: column; gap: 2px; width: 100%; }
  .perm-row {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 6px 2px; border-top: 1px solid var(--line-2);
  }
  .perm-row:first-child { border-top: none; }
  .perm-name { color: var(--text-2); }
  .chip-row { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip {
    display: inline-flex; align-items: center; gap: 6px;
    background: var(--surface-2); border: 1px solid var(--line-3);
    border-radius: var(--radius-pill); padding: 3px 6px 3px 10px; font-size: 11px; color: var(--text-2);
  }
  .chip-x {
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; color: var(--text-3); cursor: pointer; padding: 2px;
  }
  .chip-x:hover { color: var(--bad); }
  .tools-pane {
    display: flex; flex-direction: column; gap: 8px;
    border-top: 1px solid var(--line-2); padding-top: 10px; margin-top: 2px;
  }
  .tool-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .tool-head-actions { display: inline-flex; gap: 10px; }
  .tool-row {
    display: flex; align-items: center; gap: 12px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-2); padding: 8px 12px;
  }
  .tool-row .field-info { gap: 2px; }
  .perm-ico { display: inline-flex; vertical-align: -2px; margin-right: 4px; color: var(--text-3); }
</style>
