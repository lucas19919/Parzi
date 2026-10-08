<script lang="ts">
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ParziConfig, type ProviderEntry, type ProviderStatus } from "../api";
  import { board, checking, ensureBoard, refreshBoard } from "../providerStore";
  import { PROVIDER_ORDER, effortLabel, isUsable, modelLabel, nameOf, stateLabel } from "../providerRows";
  import Switch from "./Switch.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  const PROGRAM: Record<string, string> = {
    claude: "claude",
    codex: "codex",
    opencode: "opencode",
    grok: "grok",
    antigravity: "agy_acp_server",
    cursor: "cursor-agent",
  };

  let cfg: ParziConfig | null = null;
  let loading = true;
  let err = "";
  let query = "";
  let expanded = new Set<string>();
  let paths: Record<string, string> = {};
  let spendUsd = "";
  let spendTok = "";

  onMount(async () => {
    try {
      const [, c] = await Promise.all([ensureBoard(), api.getConfig()]);
      cfg = c;
      paths = Object.fromEntries(PROVIDER_ORDER.map((id) => [id, c.providers?.[id]?.binary ?? ""]));
      spendUsd = c.budget?.max_cost_usd != null ? String(c.budget.max_cost_usd) : "";
      spendTok = c.budget?.max_tokens != null ? String(c.budget.max_tokens) : "";
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  $: statusOf = (id: string) => $board.find((b) => b.provider === id);
  $: favs = new Set(cfg?.favorite_models ?? []);
  $: q = query.trim().toLowerCase();
  $: modelsFor = (p: ProviderStatus | undefined) =>
    !p ? [] : !q ? p.models : p.models.filter((m) => (m.name || m.id).toLowerCase().includes(q) || m.id.toLowerCase().includes(q));
  $: entryOf = (id: string): ProviderEntry => cfg?.providers?.[id] ?? { enabled: true };
  $: allChecking = $checking.has("*");

  function badgeOf(p: ProviderStatus | undefined, enabled: boolean): { label: string; cls: string } {
    if (!enabled) return { label: "Off", cls: "key" };
    if (!p) return { label: "Not checked yet", cls: "key" };
    const cls = p.state === "ready" ? "ok" : p.state === "unchecked" || p.state === "disabled" ? "key" : "missing";
    return { label: stateLabel(p), cls };
  }

  function ago(secs: number): string {
    const s = Math.max(0, Date.now() / 1000 - secs);
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.round(s / 60)} min ago`;
    if (s < 86400) return `${Math.round(s / 3600)} h ago`;
    return `${Math.round(s / 86400)} d ago`;
  }

  function resetsIn(at?: number): string {
    if (!at) return "";
    const s = at - Date.now() / 1000;
    if (s <= 0) return "resets now";
    if (s < 3600) return `resets in ${Math.round(s / 60)} min`;
    if (s < 86400) return `resets in ${Math.round(s / 3600)} h`;
    return `resets in ${Math.round(s / 86400)} d`;
  }

  function toggle(set: Set<string>, id: string): Set<string> {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }

  async function saveCfg(msg?: string): Promise<boolean> {
    if (!cfg) return false;
    try {
      await api.saveConfig(cfg);
      cfg = cfg;
      if (msg) notify(msg);
      return true;
    } catch (e) {
      notify(`Save failed: ${e}`);
      try {
        cfg = await api.getConfig();
        paths = Object.fromEntries(PROVIDER_ORDER.map((id) => [id, cfg?.providers?.[id]?.binary ?? ""]));
      } catch {}
      return false;
    }
  }

  function setEntry(id: string, patch: Partial<ProviderEntry>) {
    if (!cfg) return;
    cfg.providers = { ...(cfg.providers ?? {}), [id]: { ...entryOf(id), ...patch } };
  }

  async function checkAgain(ids: string[] = []) {
    try {
      await refreshBoard(ids);
      if (!ids.length) notify("Asked every agent again");
    } catch (e) {
      notify(`Check failed: ${e}`);
    }
  }

  async function setEnabled(id: string, on: boolean) {
    setEntry(id, { enabled: on });
    if (await saveCfg(on ? `${nameOf(id)} is on` : `${nameOf(id)} is off: the picker leaves it out`)) {
      void checkAgain([id]);
    }
  }

  async function savePath(id: string) {
    const next = (paths[id] ?? "").trim();
    if (next === (entryOf(id).binary ?? "")) return;
    setEntry(id, { binary: next || undefined });
    if (await saveCfg(next ? `${nameOf(id)} runs ${next}` : `${nameOf(id)} runs \`${PROGRAM[id]}\` from PATH`)) {
      void checkAgain([id]);
    }
  }

  async function setDefault(id: string, model: string) {
    setEntry(id, { default_model: model });
    await saveCfg(`New ${nameOf(id)} threads start on ${model}`);
  }

  function saveSpend() {
    if (!cfg) return;
    const usd = spendUsd.trim() ? Number(spendUsd) : NaN;
    const tok = spendTok.trim() ? Number(spendTok) : NaN;
    cfg.budget = {
      max_cost_usd: Number.isFinite(usd) && usd > 0 ? usd : null,
      max_tokens: Number.isFinite(tok) && tok > 0 ? tok : null,
    };
    void saveCfg();
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
      <button class="sbtn" on:click={() => checkAgain()} disabled={allChecking} title="Ask every agent's own program where it stands. Spends no quota.">
        {allChecking ? "Checking…" : "↻ Check again"}
      </button>
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Spending limits</h3>
    <p class="section-desc">
      A run pauses when it hits either cap. Empty is no cap. A turn on a plan reports no
      dollar cost, so only the token cap stops it.
    </p>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Max spend per run</span>
        <span class="field-hint">US dollars, for agents billed per token.</span>
      </div>
      <div class="spend">
        <span class="cur">$</span>
        <input class="spend-in" inputmode="decimal" placeholder="none" bind:value={spendUsd} on:change={saveSpend} />
      </div>
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Max tokens per run</span>
        <span class="field-hint">In plus out. Empty is no cap.</span>
      </div>
      <input class="spend-in wide" inputmode="numeric" placeholder="none" bind:value={spendTok} on:change={saveSpend} />
    </div>
  </div>

  <div class="pref-section">
    <h3 class="section-title">Providers</h3>
    <p class="section-desc">
      Parzi runs each vendor's own agent with your existing sign-in. Sign in with the
      agent's own program; Parzi asks it where it stands and spends no quota doing so.
    </p>

    <div class="providers-list">
      {#each PROVIDER_ORDER as id (id)}
        {@const p = statusOf(id)}
        {@const entry = entryOf(id)}
        {@const b = badgeOf(p, entry.enabled)}
        {@const list = modelsFor(p)}
        {@const isOpen = expanded.has(id) || (q.length > 0 && list.length > 0)}
        {@const def = entry.default_model ?? ""}
        {@const busy = allChecking || $checking.has(id)}
        {#if !q || list.length}
          <div class="provider-card" class:signed-in={isUsable(p)} class:off={!entry.enabled}>
            <div class="provider-head">
              <ProviderLogo provider={id} size={20} />
              <div class="provider-title">
                <div class="provider-title-row">
                  <span class="provider-name">{nameOf(id)}</span>
                  <span class="status-badge {b.cls}">{b.label}</span>
                  {#if p?.version}<span class="version">{p.version}</span>{/if}
                </div>
                <span class="provider-sub">
                  {p ? `checked ${ago(p.checked_at)}` : "not checked yet"}
                </span>
              </div>
              <span class="head-spacer" />
              <button class="sbtn" disabled={busy} on:click={() => checkAgain([id])} title="Ask {nameOf(id)}'s program again">
                {busy ? "Checking…" : "Check again"}
              </button>
              <Switch on={entry.enabled} title={entry.enabled ? `Turn ${nameOf(id)} off` : `Turn ${nameOf(id)} on`} on:toggle={() => setEnabled(id, !entry.enabled)} />
            </div>

            {#if p?.hint}
              <div class="provider-how">{p.hint}</div>
            {/if}

            {#if p && !p.gated}
              <div class="provider-ungated" title="Parzi's own gate only sees what the agent asks about.">
                Can change files without asking Parzi: the folder fence cannot stop it.
              </div>
            {/if}

            {#if p?.usage?.length}
              <div class="usage">
                {#each p.usage as w (w.label)}
                  <div class="usage-row">
                    <span class="usage-label">{w.label}</span>
                    <span class="usage-bar"><span class="usage-fill" class:hot={w.used_percent >= 80} style="width:{Math.min(100, w.used_percent)}%" /></span>
                    <span class="usage-num">{Math.round(w.used_percent)}%</span>
                    {#if w.resets_at}<span class="usage-reset">{resetsIn(w.resets_at)}</span>{/if}
                  </div>
                {/each}
              </div>
            {/if}

            <div class="path-row">
              <span class="path-label">Program</span>
              <input
                class="path-in"
                placeholder="{PROGRAM[id]} (from PATH)"
                spellcheck="false"
                bind:value={paths[id]}
                on:change={() => savePath(id)}
                on:keydown={(e) => e.key === "Enter" && savePath(id)}
              />
            </div>

            {#if p && p.models.length}
              <button class="models-toggle" on:click={() => (expanded = toggle(expanded, id))}>
                <span class="disclosure">{isOpen ? "▾" : "▸"}</span>
                <span>{p.models.length} model{p.models.length === 1 ? "" : "s"}</span>
                {#if def}<span class="def-tag" title="New threads start here">◉ {def}</span>{/if}
              </button>
              {#if isOpen}
                {#if list.length}
                  <div class="model-list">
                    {#each list as m (m.id)}
                      {@const spec = `${id}/${m.id}`}
                      <div class="model-row">
                        <button class="fav-btn" title={favs.has(spec) ? "Unstar" : "Star as favorite"} on:click={() => toggleFav(spec)}>
                          {favs.has(spec) ? "★" : "☆"}
                        </button>
                        <span class="model-main">
                          <span class="model-name">{modelLabel(m)}</span>
                          <span class="model-meta">
                            <span class="mono">{m.id}</span>
                            {#if m.efforts.length}<span>effort: {m.efforts.map(effortLabel).join(" · ")}</span>{/if}
                          </span>
                        </span>
                        {#if m.is_default}<span class="pill hot" title="The agent's own default">agent default</span>{/if}
                        {#if def === m.id}
                          <span class="pill on">new threads</span>
                        {:else}
                          <button class="def-btn" title="Start new {nameOf(id)} threads on this model" on:click={() => setDefault(id, m.id)}>Default</button>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="empty-note">No models match your search.</div>
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
    background: var(--panel); border: 1px solid var(--line);
    border-radius: 8px; color: var(--muted);
  }
  .models-search input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 12.5px;
  }
  .models-search input::placeholder { color: var(--faint); }
  .clear-btn { background: transparent; border: none; color: var(--muted); cursor: pointer; font-size: 11px; padding: 0 2px; }
  .clear-btn:hover { color: var(--text); }
  .skel.search { flex: 1; min-height: 34px; }
  .skel.btn { width: 110px; min-height: 34px; }

  .mini {
    background: transparent; border: 1px solid transparent; border-radius: 5px; color: var(--muted);
    font: inherit; font-size: 9px; width: 22px; height: 20px; cursor: pointer; flex: none;
  }
  .mini:hover:not(:disabled) { background: var(--line); border-color: var(--line); color: var(--text); }
  .mini:disabled { opacity: 0.3; cursor: default; }
  .status-badge.key { background: var(--line); color: var(--muted); border: 1px solid var(--line); }

  .providers-list { display: flex; flex-direction: column; gap: 8px; }
  .provider-card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg); padding: 10px 12px;
    display: flex; flex-direction: column; gap: 6px;
  }
  .provider-card.signed-in { border-color: var(--line); }
  .provider-card.off { opacity: 0.6; }
  .provider-head { display: flex; align-items: center; gap: 10px; }
  .provider-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .provider-title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .provider-name { font-size: 13px; font-weight: 650; color: var(--text); }
  .provider-sub { font-size: 11.5px; color: var(--muted); }
  .version { font-size: 10.5px; color: var(--faint); font-family: var(--mono), ui-monospace, monospace; }
  .provider-how { font-size: 12px; color: var(--muted); line-height: 1.45; }
  .provider-ungated {
    font-size: 12px; line-height: 1.45; color: var(--warn);
    background: transparent; border: 1px solid var(--warn); border-radius: 6px; padding: 6px 9px;
  }
  .head-spacer { flex: 1; }
  .def-tag {
    font-size: 10.5px; color: var(--accent); font-family: var(--mono), ui-monospace, monospace;
    max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .usage { display: flex; flex-direction: column; gap: 5px; }
  .usage-row { display: flex; align-items: center; gap: 8px; font-size: 11.5px; color: var(--muted); }
  .usage-label { width: 96px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .usage-bar { flex: 1; height: 5px; border-radius: 3px; background: var(--line); overflow: hidden; }
  .usage-fill { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
  .usage-fill.hot { background: var(--warn); }
  .usage-num { width: 34px; text-align: right; font-variant-numeric: tabular-nums; color: var(--muted); flex: none; }
  .usage-reset { font-size: 10.5px; color: var(--faint); flex: none; }

  .path-row { display: flex; align-items: center; gap: 8px; }
  .path-label { font-size: 11.5px; color: var(--muted); flex: none; width: 58px; }
  .path-in {
    flex: 1; min-width: 0; background: var(--bg);
    border: 1px solid var(--line); border-radius: 6px;
    color: var(--text); font-size: 12px; padding: 5px 9px; outline: none;
    font-family: var(--mono), ui-monospace, monospace;
  }
  .path-in:focus { border-color: var(--accent); }
  .path-in::placeholder { color: var(--faint); }

  .models-toggle {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: transparent; border: none; cursor: pointer; padding: 2px 0 0;
    font: inherit; font-size: 11.5px; text-align: left; color: var(--muted);
  }
  .models-toggle:hover { color: var(--text); }
  .disclosure { font-size: 10px; width: 10px; flex: none; text-align: center; }

  .model-list {
    display: flex; flex-direction: column; gap: 2px;
    border-top: 1px solid var(--line); padding-top: 8px;
    max-height: 320px; overflow-y: auto;
  }
  .model-row {
    display: flex; align-items: center; gap: 8px; padding: 5px 8px;
    border-radius: 6px; font-size: 12px;
  }
  .model-row:hover { background: var(--line); }
  .fav-btn {
    background: transparent; border: none; color: var(--muted); cursor: pointer;
    font-size: 13px; padding: 0 2px; flex: none; line-height: 1;
  }
  .fav-btn:hover { color: var(--warn); }
  .model-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .model-name { color: var(--text); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .model-meta { display: flex; align-items: center; gap: 7px; font-size: 10.5px; color: var(--faint); overflow: hidden; white-space: nowrap; }
  .model-meta .mono { font-family: var(--mono), ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; }
  .pill {
    font-size: 9px; font-weight: 600; padding: 1px 5px; border-radius: 4px; flex: none;
    background: var(--line); border: 1px solid var(--line); color: var(--muted);
  }
  .pill.hot { background: var(--line); border-color: var(--accent); color: var(--accent); }
  .pill.on { background: transparent; border-color: var(--ok); color: var(--ok); }
  .def-btn {
    background: transparent; border: 1px solid transparent; border-radius: 5px;
    color: var(--faint); font: inherit; font-size: 10.5px; cursor: pointer; padding: 3px 8px; flex: none;
  }
  .def-btn:hover { color: var(--text); border-color: var(--line); background: var(--line); }
  .empty-note { padding: 10px; font-size: 12px; color: var(--faint); }
  .spend { display: flex; align-items: center; gap: 6px; }
  .cur { font-size: 13px; color: var(--muted); }
  .spend-in {
    width: 88px; background: var(--bg); border: 1px solid var(--line); border-radius: 6px;
    color: var(--text); font: inherit; font-size: 13px; padding: 6px 8px; outline: none;
    font-variant-numeric: tabular-nums;
  }
  .spend-in.wide { width: 120px; }
  .spend-in:focus { border-color: var(--accent); }
</style>
