<script lang="ts">
  import Icon from "../Icon.svelte";
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ParziConfig, type ProviderEntry, type ProviderStatus, type RegistryAgent } from "../api";
  import { formatCommand, splitCommand } from "../acpCommand";
  import { board, checking, ensureBoard, nextStep, refreshBoard, refreshStale, setupWaits } from "../providerStore";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "../providerRows";
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
  let cfg: ParziConfig | null = null;
  let loading = true;
  let err = "";
  let open = new Set<string>();
  let paths: Record<string, string> = {};
  let registry: RegistryAgent[] = [];
  let registryErr = "";
  let registryBusy = false;
  let acpLabel = "";
  const AGENT_ORDER = [...PROVIDER_ORDER, "acp"];

  onMount(async () => {
    try {
      const [, c] = await Promise.all([ensureBoard(), api.getConfig()]);
      cfg = c;
      paths = pathsOf(c);
      acpLabel = c.providers?.acp?.label ?? "";
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

  const pathsOf = (c: ParziConfig) =>
    Object.fromEntries(
      AGENT_ORDER.map((id) => [
        id,
        id === "acp"
          ? formatCommand(c.providers?.[id]?.binary ?? "", c.providers?.[id]?.args ?? [])
          : (c.providers?.[id]?.binary ?? ""),
      ]),
    );

  function statusLine(p: ProviderStatus | undefined, enabled: boolean, busy: boolean): string {
    if (!enabled) return "Off";
    if (!p || p.state === "unchecked" || p.state === "disabled") return busy ? "Checking…" : "Not checked yet";
    if (p.state === "error") return p.hint || "Check failed";
    const parts = [stateLabel(p)];
    if (p.version && p.state === "ready") parts.push(p.version);
    return parts.join(" · ");
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
      acpLabel = cfg.providers?.acp?.label ?? "";
      if (msg) notify(msg);
      return true;
    } catch (e) {
      const declined = String(e).includes("cancelled");
      notify(declined ? "Not saved" : `Could not save: ${e}`, !declined);
      try {
        cfg = await api.getConfig();
        paths = pathsOf(cfg);
        acpLabel = cfg.providers?.acp?.label ?? "";
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

  function sameArgs(a: string[], b: string[]): boolean {
    return a.length === b.length && a.every((part, i) => part === b[i]);
  }

  async function savePath(id: string) {
    if (id === "acp") {
      const { binary, args } = splitCommand(paths.acp ?? "");
      const cur = entryOf(id);
      if (binary === (cur.binary ?? "") && sameArgs(args, cur.args ?? [])) return;
      if (await update(patchEntry(id, { binary: binary || undefined, args }))) {
        if (cfg) paths = pathsOf(cfg);
        void recheck([id]);
      }
      return;
    }
    const next = (paths[id] ?? "").trim();
    if (next === (entryOf(id).binary ?? "")) return;
    if (await update(patchEntry(id, { binary: next || undefined }))) void recheck([id]);
  }

  async function saveAcpLabel() {
    const next = acpLabel.trim();
    if (next === (entryOf("acp").label ?? "")) return;
    if (await update(patchEntry("acp", { label: next }))) void recheck(["acp"]);
  }

  async function loadRegistry() {
    registryBusy = true;
    registryErr = "";
    try {
      registry = await api.acpRegistry();
    } catch (e) {
      registryErr = String(e);
    } finally {
      registryBusy = false;
    }
  }

  async function useRegistry(row: RegistryAgent) {
    paths = { ...paths, acp: formatCommand(row.command, row.args) };
    acpLabel = row.name;
    if (await update(patchEntry("acp", { enabled: true, binary: row.command, args: row.args, label: row.name }), `ACP runs ${row.name}`)) {
      if (cfg) paths = pathsOf(cfg);
      void recheck(["acp"]);
    }
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
    {#each AGENT_ORDER as id (id)}
        {@const p = statusOf(id)}
        {@const entry = entryOf(id)}
        {@const busy = allChecking || $checking.has(id)}
        {@const wait = $setupWaits.get(id)}
        {@const rawStep = entry.enabled && !wait ? nextStep(p) : null}
        
        {@const step = id === "acp" && rawStep?.label !== "Try again" ? null : rawStep}
        {@const ready = entry.enabled && isUsable(p)}
        {@const isOpen = open.has(id)}
        <div class="rows" class:off={!entry.enabled}>
          <div class="row">
            <ProviderLogo provider={id} size={20} />
            <span class="row-label">{nameOf(id)}{#if id === "acp" && entry.label} · {entry.label}{/if}
              <small class:ok={ready && p?.state === "ready"} title={p?.state === "error" ? (p.hint ?? "") : ""}>{wait?.note ?? statusLine(p, entry.enabled, busy)}</small>
            </span>
            {#if ready && p && !p.gated}
              <span class="chip warn" use:tooltip={"It changes files on its own. Parzi cannot ask you first or keep it inside the project folder."}>Edits without asking</span>
            {/if}
            <span class="row-ctl auto">
              {#if step}
                <button class="sbtn primary" disabled={busy} on:click={() => step.run(id)}>{busy && step.label === "Try again" ? "Checking…" : step.label}</button>
              {:else if wait}
                <span class="waiting">{wait.action === "install" ? "Installing…" : "Signing in…"}</span>
              {/if}
              <Switch on={entry.enabled} title={entry.enabled ? `Turn ${nameOf(id)} off` : `Turn ${nameOf(id)} on`} on:toggle={() => setEnabled(id, !entry.enabled)} />
            </span>
          </div>

          {#if wait}
            {#key wait.note}
              <div class="bar"><span class="progress" class:sweep={!wait.eta} style:--eta="{wait.eta}ms"><span /></span></div>
            {/key}
          {/if}

          {#if ready && p?.usage?.length}
            {#each p.usage as w (w.label)}
              <div class="row">
                <span class="usage-label">{w.label}</span>
                <span class="usage-bar"><span class="usage-fill" class:hot={w.used_percent >= 80} style="width:{Math.min(100, w.used_percent)}%" /></span>
                <span class="usage-num">{Math.round(w.used_percent)}%</span>
                {#if w.resets_at}<span class="usage-reset">{resetsIn(w.resets_at)}</span>{/if}
              </div>
            {/each}
          {/if}

          <button class="row as-toggle" aria-expanded={isOpen} on:click={() => toggleOpen(id)}>
            <span class="disclosure"><Icon name={isOpen ? "chevDown" : "chevRight"} size={12} /></span>
            <span>Program</span>
          </button>

          {#if isOpen}
            {#if id === "acp"}
              <label class="row">
                <span class="row-label">Name</span>
                <input
                  class="tin fixed"
                  placeholder="ACP"
                  aria-label="ACP name"
                  spellcheck="false"
                  bind:value={acpLabel}
                  on:change={saveAcpLabel}
                  on:keydown={(e) => e.key === "Enter" && saveAcpLabel()}
                />
              </label>
            {/if}
              <label class="row">
                <span class="row-label">{id === "acp" ? "Command" : "Program"}</span>
                <input
                  class="tin mono"
                  placeholder={id === "acp" ? "gemini --acp" : `Found automatically (${PROGRAM[id]})`}
                aria-label={id === "acp" ? "ACP command" : `${nameOf(id)} program path`}
                spellcheck="false"
                bind:value={paths[id]}
                on:change={() => savePath(id)}
                on:keydown={(e) => e.key === "Enter" && savePath(id)}
              />
            </label>
            {#if id === "acp"}
              <div class="row">
                <button class="sbtn" type="button" disabled={registryBusy} on:click={loadRegistry}>
                  {registryBusy ? "Loading registry…" : "Add from registry"}
                </button>
              </div>
              {#if registryErr}
                <div class="row"><span class="row-label bad">{registryErr}</span></div>
              {/if}
              {#each registry as row (row.id)}
                <button class="row" type="button" on:click={() => useRegistry(row)}>
                  <span class="row-label">{row.name}<small class="mono">{formatCommand(row.command, row.args)}</small></span>
                </button>
              {/each}
            {/if}
          {/if}
        </div>
      {/each}
  </div>
{/if}

<style>
  .rows.off .row:first-child > :not(.row-ctl),
  .rows.off .as-toggle {
    opacity: 0.55;
  }
  .row .chip {
    flex: none;
    margin-left: calc(var(--space) * 2);
  }
  /* Rows inherit the muted secondary tone; names and values opt back in. */
  .rows .row {
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .chip {
    font-size: var(--fs-xs); font-weight: 600; padding: var(--space) calc(var(--space) * 2); border-radius: 999px; white-space: nowrap; cursor: default;
  }
  .chip.warn { color: var(--warn); border: 1px solid var(--warn); }
  .waiting { font-size: var(--fs-md); color: var(--muted); white-space: nowrap; }

  /* Hairline progress strip between rows while a provider sets up. */
  .rows > .bar { margin: 0; }
  .progress { display: block; height: 3px; background: var(--line); overflow: hidden; }
  .progress > span {
    display: block; height: 100%; width: 100%; background: var(--accent);
    transform-origin: left; animation: fill var(--eta) linear forwards;
  }
  .progress.sweep > span { width: 30%; animation: sweep 1.4s ease-in-out infinite; }
  @keyframes fill { from { transform: scaleX(0); } to { transform: scaleX(0.95); } }
  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  @media (prefers-reduced-motion: reduce) {
    .progress > span, .progress.sweep > span { animation: none; }
  }

  .usage-label { width: 96px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-label.bad { color: var(--bad); }
  .usage-bar { flex: 1; min-width: 40px; height: 5px; border-radius: var(--radius-sm); background: var(--line); overflow: hidden; }
  .usage-fill { display: block; height: 100%; background: var(--accent); border-radius: var(--radius-sm); }
  .usage-fill.hot { background: var(--warn); }
  .usage-num { width: 34px; text-align: right; font-variant-numeric: tabular-nums; color: var(--muted); flex: none; }
  .usage-reset { font-size: var(--fs-md); color: var(--muted); flex: none; }

  .as-toggle { color: var(--muted); }
  .as-toggle:hover { color: var(--text); }
  .disclosure { display: inline-flex; flex: none; }
</style>
