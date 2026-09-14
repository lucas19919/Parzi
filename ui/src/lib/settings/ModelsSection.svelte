<script lang="ts">
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ModelRow, type ParziConfig } from "../api";
  import { modelRows, ensureModels, refreshModels } from "../modelStore";
  import Switch from "./Switch.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  /** Roster in display order; mirrors the backend `PROVIDERS` list. */
  interface Meta {
    id: string;
    name: string;
    plan: string;
    how: string;
    keyLabel: string | null;
    keyHint: string;
    routes: boolean;
  }
  const META: Meta[] = [
    {
      id: "claude",
      name: "Claude",
      plan: "Claude Max / Pro through the Claude Code CLI",
      how: "Run `claude` once and sign in. Parzi reads the CLI's token read-only and refreshes nothing.",
      keyLabel: "Anthropic API key",
      keyHint: "Pay-per-token fallback when no Claude Code sign-in is present.",
      routes: true,
    },
    {
      id: "codex",
      name: "Codex",
      plan: "ChatGPT plan through the Codex CLI",
      how: "Run `codex login`. Parzi reads ~/.codex/auth.json read-only.",
      keyLabel: "OpenAI API key",
      keyHint: "Pay-per-token fallback when no Codex sign-in is present.",
      routes: true,
    },
    {
      id: "antigravity",
      name: "Antigravity",
      plan: "Google account — Gemini and Claude through Antigravity",
      how: "Unofficial path, opt-in. Use an account you can afford to lose.",
      keyLabel: null,
      keyHint: "",
      routes: true,
    },
    {
      id: "opencode",
      name: "OpenCode",
      plan: "Your opencode account through `opencode serve`",
      how: "Start `opencode serve` (localhost:4096). Whatever opencode routes to shows up here.",
      keyLabel: "OpenCode API key",
      keyHint: "Only needed when your serve instance requires a bearer.",
      routes: true,
    },
    {
      id: "xai",
      name: "Grok",
      plan: "xAI API key — there is no flat-rate plan",
      how: "Paste an xAI key, or sign in with the Grok CLI and Parzi picks it up.",
      keyLabel: "xAI API key",
      keyHint: "Explicit picks only unless keys may join Smart Auto.",
      routes: false,
    },
  ];
  const metaOf = (id: string) => META.find((m) => m.id === id);

  let cfg: ParziConfig | null = null;
  let loading = true;
  let refreshing = false;
  let err = "";
  let query = "";
  let expanded = new Set<string>();
  let keyOpen = new Set<string>();
  let keys: Record<string, string> = {};
  let showKey: Record<string, boolean> = {};
  let signingInGoogle = false;

  onMount(async () => {
    try {
      const [, c] = await Promise.all([ensureModels(false), api.getConfig()]);
      cfg = c;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  $: rows = $modelRows;
  $: rowOf = (id: string) => rows.find((r) => r.provider === id);
  $: favs = new Set(cfg?.favorite_models ?? []);
  $: q = query.trim().toLowerCase();
  $: modelsFor = (r: ModelRow) =>
    !q
      ? r.models
      : r.models.filter(
          (m) => (m.name || m.id).toLowerCase().includes(q) || m.id.toLowerCase().includes(q),
        );
  $: defaultFor = (provider: string) => cfg?.providers?.[provider]?.default_model ?? "";

  /** Config order, normalised to the routable roster, then the rest. */
  $: autoOrder = (() => {
    const base = (cfg?.routing?.auto_order ?? []).filter((p) => metaOf(p)?.routes);
    for (const m of META) if (m.routes && !base.includes(m.id)) base.push(m.id);
    return base;
  })();
  $: keysInAuto = cfg?.routing?.keys_in_auto ?? false;
  $: autoFailover = cfg?.routing?.auto_failover ?? true;

  function statusOf(r: ModelRow | undefined): { label: string; cls: string } {
    if (!r) return { label: "Unavailable", cls: "missing" };
    if (r.auth === "expired") return { label: "Expired", cls: "expired" };
    if (r.auth !== "ok") return { label: "Not signed in", cls: "missing" };
    if (r.billing === "subscription") return { label: r.account || "Subscription", cls: "ok" };
    return { label: "API key", cls: "key" };
  }

  function routesNow(r: ModelRow | undefined): boolean {
    if (!r || r.auth !== "ok") return false;
    return r.billing === "subscription" || (keysInAuto && r.billing === "api_key");
  }

  function toggle(set: Set<string>, id: string): Set<string> {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }

  function ctxLabel(n: number): string {
    if (!n) return "";
    return n >= 1_000_000 ? `${Math.round(n / 100_000) / 10}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`;
  }

  async function saveCfg(msg?: string) {
    if (!cfg) return;
    try {
      await api.saveConfig(cfg);
      cfg = cfg;
      if (msg) notify(msg);
    } catch (e) {
      notify(`Save failed: ${e}`);
      try {
        cfg = await api.getConfig();
      } catch {}
    }
  }

  function move(id: string, dir: -1 | 1) {
    if (!cfg) return;
    const order = [...autoOrder];
    const i = order.indexOf(id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    cfg.routing = { ...cfg.routing, auto_order: order };
    saveCfg();
  }

  function setKeysInAuto(on: boolean) {
    if (!cfg) return;
    cfg.routing = { ...cfg.routing, keys_in_auto: on };
    saveCfg(on ? "API keys may now join Smart Auto" : "Smart Auto is subscriptions only");
  }

  function setFailover(on: boolean) {
    if (!cfg) return;
    cfg.routing = { ...cfg.routing, auto_failover: on };
    saveCfg(on ? "Failover: adaptive" : "Failover: strict");
  }

  async function fullRefresh() {
    refreshing = true;
    try {
      await refreshModels();
      notify("Model catalog refreshed");
    } catch (e) {
      notify(`Refresh failed: ${e}`);
    } finally {
      refreshing = false;
    }
  }

  async function saveKey(provider: string) {
    const val = keys[provider];
    if (!val || !val.trim()) return;
    try {
      await api.saveKey(provider, val.trim());
      keys[provider] = "";
      keyOpen = toggle(keyOpen, provider);
      notify(`Saved ${metaOf(provider)?.keyLabel ?? "API key"} to the OS keyring`);
      await refreshModels();
    } catch (e) {
      notify(`Error saving key: ${e}`);
    }
  }

  async function removeKey(provider: string) {
    try {
      await api.deleteKey(provider);
      notify(`Removed ${metaOf(provider)?.keyLabel ?? "API key"}`);
      await refreshModels();
    } catch (e) {
      notify(`Error removing key: ${e}`);
    }
  }

  async function setDefault(provider: string, id: string) {
    if (!cfg) return;
    const prev = cfg.providers?.[provider];
    cfg.providers = {
      ...(cfg.providers ?? {}),
      [provider]: { default_model: id, ...(prev?.base_url ? { base_url: prev.base_url } : {}) },
    };
    await saveCfg(`Default for ${metaOf(provider)?.name ?? provider}: ${id}`);
  }

  async function toggleFav(spec: string) {
    try {
      const list = await api.toggleFavorite(spec);
      if (cfg) {
        cfg.favorite_models = list;
        cfg = cfg;
      }
    } catch (e) {
      notify(`Favorite failed: ${e}`);
    }
  }

  async function signInGoogle() {
    signingInGoogle = true;
    notify("Opening Google sign-in in your browser…");
    try {
      const res = await api.loginAntigravity();
      notify(res || "Signed in with Google");
      await refreshModels();
    } catch (e) {
      notify(`Sign-in failed: ${e}`);
    } finally {
      signingInGoogle = false;
    }
  }

  async function signOutGoogle() {
    try {
      await api.logoutAntigravity();
      notify("Signed out of Google");
      await refreshModels();
    } catch (e) {
      notify(`Sign-out error: ${e}`);
    }
  }
</script>

{#if loading}
  <div class="pref-section">
    <div class="models-toolbar"><div class="skel search" /><div class="skel btn" /></div>
    <div class="skel tall" />
    <div class="skel" />
    <div class="skel" />
  </div>
{:else if err}
  <div class="load-err"><span>{err}</span></div>
{:else}
  <div class="pref-section">
    <div class="models-toolbar">
      <div class="models-search">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3" /></svg>
        <input placeholder="Search models…" bind:value={query} />
        {#if query}<button class="clear-btn" on:click={() => (query = "")}>✕</button>{/if}
      </div>
      <button class="sbtn" on:click={fullRefresh} disabled={refreshing} title="Re-read sign-ins and pull live model lists">
        {refreshing ? "Refreshing…" : "↻ Refresh"}
      </button>
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Smart Auto</h3>
    <p class="section-desc">
      Auto walks your signed-in subscriptions in this order and fails over on rate limits.
      API keys are never spent unless you say so below.
    </p>
    <div class="order-card">
      {#each autoOrder as id, i (id)}
        {@const r = rowOf(id)}
        {@const st = statusOf(r)}
        {@const live = routesNow(r)}
        <div class="order-row" class:dim={!live}>
          <span class="order-n">{i + 1}</span>
          <ProviderLogo provider={id} size={15} />
          <span class="order-name">{metaOf(id)?.name ?? id}</span>
          <span class="status-badge {st.cls}">{live ? st.label : r?.auth === "ok" ? "key · skipped" : st.label}</span>
          <span class="head-spacer" />
          <button class="mini" title="Move up" disabled={i === 0} on:click={() => move(id, -1)}>▲</button>
          <button class="mini" title="Move down" disabled={i === autoOrder.length - 1} on:click={() => move(id, 1)}>▼</button>
        </div>
      {/each}
      {#if keysInAuto}
        {@const r = rowOf("xai")}
        {@const st = statusOf(r)}
        <div class="order-row" class:dim={!routesNow(r)}>
          <span class="order-n">{autoOrder.length + 1}</span>
          <ProviderLogo provider="xai" size={15} />
          <span class="order-name">Grok</span>
          <span class="status-badge {st.cls}">{st.label}</span>
          <span class="head-spacer" />
          <span class="order-note">keys, last</span>
        </div>
      {/if}
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Let API keys join Smart Auto</span>
        <span class="field-hint">Off: keys are explicit picks only. On: keyed providers queue up after every subscription.</span>
      </div>
      <Switch on={keysInAuto} title="Allow API keys in Smart Auto and failover" on:toggle={() => setKeysInAuto(!keysInAuto)} />
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Fail over on rate limits</span>
        <span class="field-hint">Adaptive: an explicit pick hops to the next provider in the order above on a 429. Strict: halt instead.</span>
      </div>
      <Switch on={autoFailover} title="Fail over to another provider on rate limits" on:toggle={() => setFailover(!autoFailover)} />
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Providers</h3>
    <p class="section-desc">
      Subscriptions come from the CLIs you already use; Parzi only reads their sign-ins.
      Keys are stored in the OS keyring.
    </p>

    <div class="providers-list">
      {#each META as meta (meta.id)}
        {@const row = rowOf(meta.id)}
        {@const st = statusOf(row)}
        {@const list = row ? modelsFor(row) : []}
        {@const isOpen = expanded.has(meta.id) || (q.length > 0 && list.length > 0)}
        {@const def = defaultFor(meta.id)}
        {@const hasKey = row?.auth === "ok" && row.billing === "api_key"}
        {#if !q || list.length}
          <div class="provider-card" class:signed-in={row?.auth === "ok"}>
            <div class="provider-head">
              <ProviderLogo provider={meta.id} size={20} />
              <div class="provider-title">
                <div class="provider-title-row">
                  <span class="provider-name">{meta.name}</span>
                  <span class="status-badge {st.cls}">{st.label}</span>
                </div>
                <span class="provider-sub">{meta.plan}</span>
              </div>
              <span class="head-spacer" />
              {#if meta.id === "antigravity"}
                {#if row?.auth === "ok"}
                  <button class="sbtn danger" on:click={signOutGoogle}>Sign out</button>
                {:else}
                  <button class="sbtn primary" on:click={signInGoogle} disabled={signingInGoogle}>
                    {signingInGoogle ? "Waiting for browser…" : "Sign in with Google"}
                  </button>
                {/if}
              {:else if meta.keyLabel}
                {#if hasKey}
                  <button class="sbtn danger" on:click={() => removeKey(meta.id)}>Remove key</button>
                {:else}
                  <button class="sbtn" class:primary={row?.auth !== "ok" && meta.id === "xai"} on:click={() => (keyOpen = toggle(keyOpen, meta.id))}>
                    {keyOpen.has(meta.id) ? "Cancel" : row?.billing === "subscription" ? "Add key anyway" : "Add API key"}
                  </button>
                {/if}
              {/if}
            </div>

            <div class="provider-how">
              {#if row?.auth === "ok"}
                {#if row.billing === "subscription"}
                  Routes on your plan{row.account ? ` (${row.account})` : ""}. Smart Auto may use it.
                {:else}
                  Pay-per-token key in use. Explicit picks only{keysInAuto ? ", plus Smart Auto (opt-in on)" : ""}.
                {/if}
              {:else if row?.auth === "expired"}
                {row.hint}
              {:else}
                {meta.how}
              {/if}
            </div>

            {#if keyOpen.has(meta.id) && meta.keyLabel}
              <div class="key-input-row">
                <div class="input-with-eye">
                  {#if showKey[meta.id]}
                    <input type="text" placeholder={meta.keyLabel} bind:value={keys[meta.id]} on:keydown={(e) => e.key === "Enter" && saveKey(meta.id)} />
                  {:else}
                    <input type="password" placeholder={meta.keyLabel} bind:value={keys[meta.id]} on:keydown={(e) => e.key === "Enter" && saveKey(meta.id)} />
                  {/if}
                  <button class="eye-btn" title={showKey[meta.id] ? "Hide key" : "Show key"} on:click={() => (showKey[meta.id] = !showKey[meta.id])}>
                    {showKey[meta.id] ? "Hide" : "Show"}
                  </button>
                </div>
                <button class="sbtn primary" disabled={!keys[meta.id]?.trim()} on:click={() => saveKey(meta.id)}>Save</button>
              </div>
              <span class="key-hint">{meta.keyHint}</span>
            {/if}

            {#if row}
              <button class="models-toggle" on:click={() => (expanded = toggle(expanded, meta.id))}>
                <span class="disclosure">{isOpen ? "▾" : "▸"}</span>
                <span>{row.models.length} model{row.models.length === 1 ? "" : "s"}</span>
                {#if def}<span class="def-tag" title="Default model">◉ {def}</span>{/if}
              </button>
              {#if isOpen}
                {#if list.length}
                  <div class="model-list">
                    {#each list as m (m.id)}
                      {@const spec = `${meta.id}/${m.id}`}
                      <div class="model-row" class:legacy={m.legacy}>
                        <button class="fav-btn" title={favs.has(spec) ? "Unstar" : "Star as favorite"} on:click={() => toggleFav(spec)}>
                          {favs.has(spec) ? "★" : "☆"}
                        </button>
                        <span class="model-main">
                          <span class="model-name">{m.name || m.id}</span>
                          <span class="model-meta">
                            <span class="mono">{m.id}</span>
                            {#if m.context_limit}<span>ctx {ctxLabel(m.context_limit)}</span>{/if}
                            {#if row.billing !== "subscription" && (m.price_in > 0 || m.price_out > 0)}<span>${m.price_in}/${m.price_out} MTok</span>{/if}
                            {#if m.vision}<span class="pill">vision</span>{/if}
                            {#if m.legacy}<span class="pill dim">legacy</span>{/if}
                          </span>
                        </span>
                        {#if m.is_default}<span class="pill hot" title="Provider flagship">flagship</span>{/if}
                        {#if def === m.id}
                          <span class="pill on">default</span>
                        {:else}
                          <button class="def-btn" title="Make default for {meta.name}" on:click={() => setDefault(meta.id, m.id)}>Default</button>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="empty-note">{q ? "No models match your search." : "No models listed yet — hit Refresh for a live pull."}</div>
                {/if}
              {/if}
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </div>
{/if}

<style>
  .models-toolbar { display: flex; gap: 8px; align-items: center; }
  .models-search {
    flex: 1; display: flex; align-items: center; gap: 7px; padding: 7px 10px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: 8px; color: var(--text-3);
  }
  .models-search input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 12.5px;
  }
  .models-search input::placeholder { color: var(--text-4); }
  .clear-btn { background: transparent; border: none; color: var(--text-3); cursor: pointer; font-size: 11px; padding: 0 2px; }
  .clear-btn:hover { color: var(--text); }
  .skel.search { flex: 1; min-height: 34px; }
  .skel.btn { width: 110px; min-height: 34px; }

  .order-card {
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-3); padding: 4px 6px; display: flex; flex-direction: column;
  }
  .order-row { display: flex; align-items: center; gap: 9px; padding: 7px 8px; border-radius: 7px; }
  .order-row + .order-row { border-top: 1px solid var(--line-2); }
  .order-row.dim .order-name { color: var(--text-3); }
  .order-n { width: 14px; font-size: 11px; color: var(--text-4); font-family: var(--parzi-mono), ui-monospace, monospace; text-align: right; flex: none; }
  .order-name { font-size: 12.5px; font-weight: 600; color: var(--text); }
  .order-note { font-size: 10.5px; color: var(--text-4); }
  .mini {
    background: transparent; border: 1px solid transparent; border-radius: 5px; color: var(--text-3);
    font: inherit; font-size: 9px; width: 22px; height: 20px; cursor: pointer; flex: none;
  }
  .mini:hover:not(:disabled) { background: var(--surface-2); border-color: var(--line-3); color: var(--text); }
  .mini:disabled { opacity: 0.3; cursor: default; }
  .status-badge.key { background: var(--surface-2); color: var(--text-2); border: 1px solid var(--line-3); }

  .providers-list { display: flex; flex-direction: column; gap: 8px; }
  .provider-card {
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: var(--radius-3); padding: 12px 14px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .provider-card.signed-in { border-color: var(--line-3); }
  .provider-head { display: flex; align-items: center; gap: 10px; }
  .provider-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .provider-title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .provider-name { font-size: 13.5px; font-weight: 700; color: var(--text); }
  .provider-sub { font-size: 11.5px; color: var(--text-3); }
  .provider-how { font-size: 12px; color: var(--text-2); line-height: 1.45; }
  .head-spacer { flex: 1; }
  .def-tag {
    font-size: 10.5px; color: var(--accent-text); font-family: var(--parzi-mono), ui-monospace, monospace;
    max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .key-input-row { display: flex; gap: 8px; }
  .key-hint { font-size: 11px; color: var(--text-4); margin-top: -4px; }
  .input-with-eye { flex: 1; position: relative; display: flex; align-items: center; min-width: 0; }
  .input-with-eye input {
    width: 100%; background: var(--input);
    border: 1px solid var(--line-2); border-radius: 6px;
    color: var(--text); font-size: 12px; padding: 6px 48px 6px 10px; outline: none;
    font-family: var(--parzi-mono), ui-monospace, monospace;
  }
  .input-with-eye input:focus { border-color: var(--accent-line); }
  .eye-btn {
    position: absolute; right: 8px; background: transparent; border: none;
    color: var(--text-4); font-size: 11px; cursor: pointer; font-family: inherit;
  }
  .eye-btn:hover { color: var(--text); }

  .models-toggle {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: transparent; border: none; cursor: pointer; padding: 2px 0 0;
    font: inherit; font-size: 11.5px; text-align: left; color: var(--text-3);
  }
  .models-toggle:hover { color: var(--text); }
  .disclosure { font-size: 10px; width: 10px; flex: none; text-align: center; }

  .model-list {
    display: flex; flex-direction: column; gap: 2px;
    border-top: 1px solid var(--line-2); padding-top: 8px;
    max-height: 320px; overflow-y: auto;
  }
  .model-row {
    display: flex; align-items: center; gap: 8px; padding: 6px 8px;
    border-radius: 6px; font-size: 12px;
  }
  .model-row:hover { background: var(--surface-2); }
  .model-row.legacy { opacity: 0.55; }
  .fav-btn {
    background: transparent; border: none; color: var(--text-3); cursor: pointer;
    font-size: 13px; padding: 0 2px; flex: none; line-height: 1;
  }
  .fav-btn:hover { color: var(--warn); }
  .model-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .model-name { color: var(--text); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .model-meta { display: flex; align-items: center; gap: 7px; font-size: 10.5px; color: var(--text-4); overflow: hidden; white-space: nowrap; }
  .model-meta .mono { font-family: var(--parzi-mono), ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; }
  .pill {
    font-size: 9px; font-weight: 600; padding: 1px 5px; border-radius: 4px; flex: none;
    background: var(--surface-2); border: 1px solid var(--line-3); color: var(--text-3);
  }
  .pill.dim { opacity: 0.7; }
  .pill.hot { background: var(--accent-mid); border-color: var(--accent-line); color: var(--accent-text); }
  .pill.on { background: var(--ok-soft); border-color: var(--ok-line); color: var(--ok); }
  .def-btn {
    background: transparent; border: 1px solid transparent; border-radius: 5px;
    color: var(--text-4); font: inherit; font-size: 10.5px; cursor: pointer; padding: 3px 8px; flex: none;
  }
  .def-btn:hover { color: var(--text); border-color: var(--line-3); background: var(--surface-2); }
  .empty-note { padding: 10px; font-size: 12px; color: var(--text-4); }
</style>
