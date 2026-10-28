<script lang="ts">
  import Icon from "../Icon.svelte";
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ParziConfig, type ProviderEntry, type ProviderStatus } from "../api";
  import { board, checking, ensureBoard, nextStep, refreshBoard, refreshStale, setupWaits } from "../providerStore";
  import { PROVIDER_ORDER, effortLabel, isUsable, modelLabel, nameOf, stateLabel } from "../providerRows";
  import Switch from "./Switch.svelte";
  import { updateConfig } from "../config";
  import { tooltip } from "../tooltip";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  const PROGRAM: Record<string, string> = {
    claude: "claude",
    codex: "codex",
    opencode: "opencode",
    grok: "grok",
    antigravity: "agy_acp_server",
    cursor: "cursor-agent",
  };
  // A model list this long gets its own search.
  const SEARCH_FROM = 12;

  let cfg: ParziConfig | null = null;
  let loading = true;
  let err = "";
  let open = new Set<string>();
  let queries: Record<string, string> = {};
  let paths: Record<string, string> = {};

  onMount(async () => {
    try {
      const [, c] = await Promise.all([ensureBoard(), api.getConfig()]);
      cfg = c;
      paths = pathsOf(c);
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
    void refreshStale().catch(() => {});
  });

  $: statusOf = (id: string) => $board.find((b) => b.provider === id);
  $: entryOf = (id: string): ProviderEntry => cfg?.providers?.[id] ?? { enabled: true };
  $: allChecking = $checking.has("*");

  const pathsOf = (c: ParziConfig) => Object.fromEntries(PROVIDER_ORDER.map((id) => [id, c.providers?.[id]?.binary ?? ""]));

  function statusLine(p: ProviderStatus | undefined, enabled: boolean, busy: boolean): string {
    if (!enabled) return "Off";
    // Switched on again, but last checked while off: due for a check.
    if (!p || p.state === "unchecked" || p.state === "disabled") return busy ? "Checking…" : "Not checked yet";
    const parts = [stateLabel(p)];
    if (p.version && p.state === "ready") parts.push(p.version);
    return parts.join(" · ");
  }

  function filtered(p: ProviderStatus | undefined, q: string) {
    const list = p?.models ?? [];
    const s = q.trim().toLowerCase();
    return s ? list.filter((m) => `${m.name} ${m.id}`.toLowerCase().includes(s)) : list;
  }

  function resetsIn(at?: number): string {
    if (!at) return "";
    const s = at - Date.now() / 1000;
    if (s <= 0) return "resets now";
    if (s < 3600) return `resets in ${Math.round(s / 60)} min`;
    if (s < 86400) return `resets in ${Math.round(s / 3600)} h`;
    return `resets in ${Math.round(s / 86400)} d`;
  }

  function toggleOpen(id: string) {
    const next = new Set(open);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    open = next;
  }

  async function update(change: (c: ParziConfig) => void, msg?: string): Promise<boolean> {
    try {
      cfg = await updateConfig(change);
      if (msg) notify(msg);
      return true;
    } catch (e) {
      const declined = String(e).includes("cancelled");
      notify(declined ? "Not saved" : `Could not save: ${e}`, !declined);
      try {
        cfg = await api.getConfig();
        paths = pathsOf(cfg);
      } catch {}
      return false;
    }
  }

  const patchEntry = (id: string, patch: Partial<ProviderEntry>) => (c: ParziConfig) => {
    c.providers = { ...(c.providers ?? {}), [id]: { ...(c.providers?.[id] ?? { enabled: true }), ...patch } };
  };

  const recheck = (ids: string[] = []) => refreshBoard(ids).catch((e) => notify(`Could not check: ${e}`, true));

  async function setEnabled(id: string, on: boolean) {
    if (await update(patchEntry(id, { enabled: on }))) void recheck([id]);
  }

  async function savePath(id: string) {
    const next = (paths[id] ?? "").trim();
    if (next === (entryOf(id).binary ?? "")) return;
    if (await update(patchEntry(id, { binary: next || undefined }))) void recheck([id]);
  }

</script>

{#if loading}
  <div class="pref-section">
    <div class="skel" />
    <div class="skel" />
    <div class="skel" />
  </div>
{:else if err}
  <div class="load-err"><span>{err}</span></div>
{:else}
  <div class="pref-section">
    <div class="section-head-with-action">
      <h3 class="section-title">Providers</h3>
      <button class="icon-btn" class:spin={allChecking} disabled={allChecking} use:tooltip={allChecking ? "Checking…" : "Check all again"} aria-label="Check all agents again" on:click={() => recheck()}>
        <Icon name="reload" size={12} />
      </button>
    </div>

    <div class="providers-list">
      {#each PROVIDER_ORDER as id (id)}
        {@const p = statusOf(id)}
        {@const entry = entryOf(id)}
        {@const busy = allChecking || $checking.has(id)}
        {@const wait = $setupWaits.get(id)}
        {@const step = entry.enabled && !wait ? nextStep(p) : null}
        {@const ready = entry.enabled && isUsable(p)}
        {@const isOpen = open.has(id)}
        <div class="provider-card" class:off={!entry.enabled}>
          <div class="provider-head">
            <ProviderLogo provider={id} size={20} />
            <div class="provider-title">
              <div class="provider-title-row">
                <span class="provider-name">{nameOf(id)}</span>
                {#if ready && p && !p.gated}
                  <span class="chip warn" use:tooltip={"It changes files on its own. Parzi cannot ask you first or keep it inside the project folder."}>Edits without asking</span>
                {/if}
              </div>
              <span class="provider-sub" class:ok={ready && p?.state === "ready"}>{wait?.note ?? statusLine(p, entry.enabled, busy)}</span>
            </div>
            <span class="head-spacer" />
            {#if step}
              <button class="sbtn primary" disabled={busy} on:click={() => step.run(id)}>{busy && step.label === "Try again" ? "Checking…" : step.label}</button>
            {:else if wait}
              <span class="waiting">{wait.action === "install" ? "Installing…" : "Signing in…"}</span>
            {/if}
            <Switch on={entry.enabled} title={entry.enabled ? `Turn ${nameOf(id)} off` : `Turn ${nameOf(id)} on`} on:toggle={() => setEnabled(id, !entry.enabled)} />
          </div>

          {#if wait}
            {#key wait.note}
              <span class="progress" class:sweep={!wait.eta} style:--eta="{wait.eta}ms"><span /></span>
            {/key}
          {/if}

          {#if entry.enabled && p?.state === "error" && p.hint}
            <div class="provider-how">{p.hint}</div>
          {:else if ready && p?.hint}
            <div class="provider-how">{p.hint}</div>
          {/if}

          {#if ready && p?.usage?.length}
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

          <button class="models-toggle" aria-expanded={isOpen} on:click={() => toggleOpen(id)}>
            <span class="disclosure"><Icon name={isOpen ? "chevDown" : "chevRight"} size={12} /></span>
            {#if p?.models.length}
              <span>{p.models.length} model{p.models.length === 1 ? "" : "s"}</span>
            {:else}
              <span>Options</span>
            {/if}
          </button>

          {#if isOpen}
            <div class="details">
              {#if p?.models.length}
              {#if p.models.length > SEARCH_FROM}
                <div class="models-search">
                  <Icon name="search" size={12} />
                  <input class="tin" placeholder="Search {p.models.length} models" aria-label="Search {nameOf(id)} models" bind:value={queries[id]} />
                </div>
              {/if}
                <div class="model-list">
                  {#each filtered(p, queries[id] ?? "") as m (m.id)}
                    <div class="model-row">
                      <span class="model-main">
                        <span class="model-name">{modelLabel(m)}{#if m.is_default}<span class="agent-def"> · default</span>{/if}</span>
                        <span class="model-meta">
                          <span class="mono">{m.id}</span>
                          {#if m.efforts.length}<span>{m.efforts.map(effortLabel).join(" · ")}</span>{/if}
                        </span>
                      </span>
                    </div>
                  {:else}
                    <div class="empty-note">No model matches.</div>
                  {/each}
                </div>
              {/if}
              <label class="path-row">
                <span class="path-label">Program</span>
                <input
                  class="tin mono"
                  placeholder="Found automatically ({PROGRAM[id]})"
                  aria-label="{nameOf(id)} program path"
                  spellcheck="false"
                  bind:value={paths[id]}
                  on:change={() => savePath(id)}
                  on:keydown={(e) => e.key === "Enter" && savePath(id)}
                />
              </label>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .icon-btn {
    width: 28px; height: 28px; flex: none; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: 1px solid var(--line); border-radius: var(--radius);
    color: var(--muted); cursor: pointer; padding: 0;
  }
  .icon-btn:hover:not(:disabled) { color: var(--text); background: var(--line); }
  .icon-btn:disabled { cursor: default; }
  .icon-btn.spin :global(svg) { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .providers-list { display: flex; flex-direction: column; gap: calc(var(--space) * 2); }
  .provider-card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg); padding: calc(var(--space) * 3) calc(var(--space) * 3);
    display: flex; flex-direction: column; gap: calc(var(--space) * 2);
  }
  .provider-card.off .provider-head > :global(:not(.switch)),
  .provider-card.off .models-toggle { opacity: 0.55; }
  .provider-head { display: flex; align-items: center; gap: calc(var(--space) * 3); }
  .provider-title { display: flex; flex-direction: column; gap: var(--space); min-width: 0; }
  .provider-title-row { display: flex; align-items: center; gap: calc(var(--space) * 2); flex-wrap: wrap; }
  .provider-name { font-size: var(--fs-md); font-weight: 650; color: var(--text); }
  .provider-sub { font-size: var(--fs-sm); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .provider-sub.ok { color: var(--ok); }
  .provider-how { font-size: var(--fs-sm); color: var(--muted); line-height: 1.45; }
  .chip {
    font-size: var(--fs-xs); font-weight: 600; padding: var(--space) calc(var(--space) * 2); border-radius: 999px; white-space: nowrap; cursor: default;
  }
  .chip.warn { color: var(--warn); border: 1px solid var(--warn); }
  .head-spacer { flex: 1; }
  .sbtn.primary { padding: var(--space) calc(var(--space) * 3); font-size: var(--fs-sm); }
  .waiting { font-size: var(--fs-sm); color: var(--muted); white-space: nowrap; }

  .progress { display: block; height: 3px; border-radius: var(--radius-sm); background: var(--line); overflow: hidden; }
  .progress > span {
    display: block; height: 100%; width: 100%; background: var(--accent); border-radius: var(--radius-sm);
    transform-origin: left; animation: fill var(--eta) linear forwards;
  }
  .progress.sweep > span { width: 30%; animation: sweep 1.4s ease-in-out infinite; }
  @keyframes fill { from { transform: scaleX(0); } to { transform: scaleX(0.95); } }
  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  @media (prefers-reduced-motion: reduce) {
    .progress > span, .progress.sweep > span, .icon-btn.spin :global(svg) { animation: none; }
  }

  .usage { display: flex; flex-direction: column; gap: var(--space); }
  .usage-row { display: flex; align-items: center; gap: calc(var(--space) * 2); font-size: var(--fs-sm); color: var(--muted); }
  .usage-label { width: 96px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .usage-bar { flex: 1; height: 5px; border-radius: var(--radius-sm); background: var(--line); overflow: hidden; }
  .usage-fill { display: block; height: 100%; background: var(--accent); border-radius: var(--radius-sm); }
  .usage-fill.hot { background: var(--warn); }
  .usage-num { width: 34px; text-align: right; font-variant-numeric: tabular-nums; color: var(--muted); flex: none; }
  .usage-reset { font-size: var(--fs-xs); color: var(--muted); flex: none; }

  .models-toggle {
    display: flex; align-items: center; gap: calc(var(--space) * 2); width: 100%; min-width: 0;
    background: transparent; border: none; cursor: pointer; min-height: 22px; padding: 0;
    font: inherit; font-size: var(--fs-sm); text-align: left; color: var(--muted);
  }
  .models-toggle:hover { color: var(--text); }
  .disclosure { display: inline-flex; flex: none; }

  .details { display: flex; flex-direction: column; gap: calc(var(--space) * 2); border-top: 1px solid var(--line); padding-top: calc(var(--space) * 2); }
  .models-search {
    display: flex; align-items: center; gap: calc(var(--space) * 2);
    background: var(--bg); border: 1px solid var(--line); border-radius: var(--radius);
    color: var(--muted); padding-left: calc(var(--space) * 3);
  }
  .models-search:focus-within { border-color: var(--accent); }
  .models-search .tin {
    flex: 1; background: transparent; border: none; border-radius: 0;
    padding-left: 0;
  }
  .models-search .tin:focus { border-color: transparent; }
  .model-list { display: flex; flex-direction: column; gap: var(--space); max-height: 340px; overflow-y: auto; }
  .model-row {
    display: flex; align-items: center; gap: calc(var(--space) * 2); padding: var(--space) calc(var(--space) * 2);
    border-radius: var(--radius); font-size: var(--fs-sm);
  }
  .model-row:hover { background: var(--line); }
  .model-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--space); }
  .model-name { color: var(--text); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .agent-def { color: var(--muted); font-weight: 400; }
  .model-meta { display: flex; align-items: center; gap: calc(var(--space) * 2); font-size: var(--fs-xs); color: var(--muted); overflow: hidden; white-space: nowrap; }
  .model-meta .mono { font-family: var(--mono), ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; }
  .empty-note { padding: calc(var(--space) * 2); font-size: var(--fs-sm); color: var(--muted); }

  .path-row { display: flex; align-items: center; gap: calc(var(--space) * 2); }
  .path-label { font-size: var(--fs-sm); color: var(--muted); flex: none; width: 58px; }
  .path-row .tin { flex: 1; }
</style>
