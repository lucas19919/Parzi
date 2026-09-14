<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ParziConfig, type Check } from "../api";
  import Switch from "./Switch.svelte";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  interface McpServer {
    command: string;
    args: string[];
    env: Record<string, string>;
    allow: string[];
    timeout_ms: number;
    enabled: boolean;
  }

  const I = {
    plus: "M12 5v14M5 12h14",
    trash: "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
    pulse: "M22 12h-4l-3 9L9 3l-3 9H2",
    edit: "M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
  };

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

  /** Zero-config servers: install with one click, no questions asked. */
  const PRESETS = [
    {
      id: "fetch",
      name: "fetch",
      label: "Fetch",
      desc: "Turn any web page into markdown.",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-fetch"],
    },
    {
      id: "memory",
      name: "memory",
      label: "Memory",
      desc: "Knowledge graph that persists across sessions.",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-memory"],
    },
  ];

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

  async function installPreset(p: { name: string; command: string; args: string[] }) {
    if (servers[p.name]) {
      notify(`${p.name} is already added`);
      return;
    }
    servers = {
      ...servers,
      [p.name]: { command: p.command, args: [...p.args], env: {}, allow: [], timeout_ms: 30_000, enabled: true },
    };
    await persist(`Added ${p.name}`);
    void probe();
  }

  let editing: string | null = null;
  let draft = { command: "", args: "", env: "", allow: "", timeout: "30" };
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

  function normalize(raw: Record<string, unknown>): Record<string, McpServer> {
    const out: Record<string, McpServer> = {};
    for (const [name, v] of Object.entries(raw)) {
      const s = (v ?? {}) as Partial<McpServer>;
      out[name] = {
        command: typeof s.command === "string" ? s.command : "",
        args: Array.isArray(s.args) ? s.args.map(String) : [],
        env: s.env && typeof s.env === "object" ? (s.env as Record<string, string>) : {},
        allow: Array.isArray(s.allow) ? s.allow.map(String) : [],
        timeout_ms: typeof s.timeout_ms === "number" ? s.timeout_ms : 30_000,
        enabled: s.enabled !== false,
      };
    }
    return out;
  }

  $: serverNames = Object.keys(servers).sort();

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

  function startEdit(name: string) {
    const s = servers[name];
    if (!s) return;
    editing = name;
    draftErr = "";
    draft = {
      command: s.command,
      args: s.args.join(" "),
      env: Object.entries(s.env).map(([k, v]) => `${k}=${v}`).join("\n"),
      allow: s.allow.join(", "),
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
      allow: draft.allow.split(",").map((a) => a.trim()).filter(Boolean),
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
  <div class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Connectors</h3>
        <p class="section-desc">MCP servers Parzi can spawn for tools. Stored in config under <b>mcp.servers</b>.</p>
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
          <span class="field-hint">Add one below — e.g. a filesystem or browser bridge.</span>
        </div>
      </div>
    {/if}

    {#each serverNames as name (name)}
      {@const s = servers[name]}
      {@const probe = probeFor(name)}
      <div class="field-card col">
        <div class="card-top">
          <div class="field-info">
            <span class="field-label">{name}
              {#if probe}
                <span class="status-badge {probe.ok ? 'ok' : 'expired'}">{probe.ok ? probe.detail : "unreachable"}</span>
              {:else}
                <span class="status-badge {s.enabled ? 'ok' : 'missing'}">{s.enabled ? "enabled" : "off"}</span>
              {/if}
            </span>
            <span class="field-hint mono">{s.command}{#if s.args.length} {s.args.join(" ")}{/if} · {Math.round(s.timeout_ms / 1000)}s{#if Object.keys(s.env).length} · {Object.keys(s.env).length} env{/if}{#if s.allow.length} · {s.allow.length} tools allowed{/if}</span>
            {#if probe && !probe.ok}
              <span class="field-hint">{probe.detail}</span>
            {/if}
          </div>
          <div class="row-actions">
            <Switch on={s.enabled} title={s.enabled ? `Disable ${name}` : `Enable ${name}`} on:toggle={() => toggleEnabled(name)} />
            <button class="mini-btn" title={editing === name ? "Close editor" : `Edit ${name}`} on:click={() => (editing === name ? (editing = null) : startEdit(name))}>
              <Icon d={I.edit} size={12} />
            </button>
            <button class="sbtn danger" title={`Remove ${name}`} on:click={() => { if (editing === name) editing = null; removeServer(name); }}>
              <Icon d={I.trash} size={12} />
            </button>
          </div>
        </div>
        {#if editing === name}
          <div class="form-grid">
            <label class="fld"><span>Command</span><input class="txt mono" bind:value={draft.command} /></label>
            <label class="fld"><span>Args (space-separated)</span><input class="txt mono" bind:value={draft.args} /></label>
            <label class="fld"><span>Env (one KEY=value per line)</span><textarea class="txt mono" rows="3" bind:value={draft.env} /></label>
            <div class="form-2col">
              <label class="fld"><span>Allowed tools (comma-separated, blank = all)</span><input class="txt mono" bind:value={draft.allow} /></label>
              <label class="fld"><span>Timeout (seconds)</span><input class="txt mono" bind:value={draft.timeout} inputmode="numeric" /></label>
            </div>
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

  <div class="pref-section">
    <h3 class="section-title">Add a connector</h3>
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
    <div class="preset-grid">
      {#each PRESETS as p (p.id)}
        <div class="preset">
          <div class="field-info">
            <span class="field-label">{p.label}</span>
            <span class="field-hint">{p.desc}</span>
          </div>
          {#if servers[p.name]}
            <span class="status-badge ok">added</span>
          {:else}
            <button class="sbtn" on:click={() => installPreset(p)}><span>Install</span></button>
          {/if}
        </div>
      {/each}
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
{/if}

<style>
  .mono { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11.5px; }
  .row-actions { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  .field-card.col { flex-direction: column; align-items: stretch; }
  .card-top { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .form-grid { display: flex; flex-direction: column; gap: 8px; }
  .form-2col { display: grid; grid-template-columns: 1fr 140px; gap: 8px; }
  .fld { display: flex; flex-direction: column; gap: 4px; }
  .fld > span { font-size: 11px; color: var(--text-3); }
  .txt {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; padding: 7px 10px; width: 100%;
  }
  textarea.txt { resize: vertical; min-height: 56px; line-height: 1.5; }
  .txt::placeholder { color: var(--text-4); }
  .paste-row { display: grid; grid-template-columns: 1fr auto; gap: 8px; }
  .preset-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .preset {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-3); padding: 10px 14px;
  }
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
</style>
