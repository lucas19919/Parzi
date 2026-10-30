<script lang="ts">
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ParziConfig, type ProviderEntry, type ProviderStatus, type RegistryAgent } from "../api";
  import { formatCommand, splitCommand } from "../acpCommand";
  import { board, checking, ensureBoard, nextStep, refreshBoard, refreshStale, setupWaits } from "../providerStore";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "../providerRows";
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

  function dotState(p: ProviderStatus | undefined, enabled: boolean, busy: boolean): string {
    if (!enabled) return "off";
    if (busy) return "warn";
    if (!p || p.state === "unchecked" || p.state === "disabled") return "idle";
    if (p.state === "error") return "bad";
    if (isUsable(p)) return "ok";
    return "warn";
  }

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
  {#each [0, 1, 2] as i (i)}
    <div class="pref-section">
      <div class="skel" />
    </div>
  {/each}
{:else if err}
  <div class="load-err"><span>{err}</span></div>
{:else}
    {#each AGENT_ORDER as id (id)}
        {@const p = statusOf(id)}
        {@const entry = entryOf(id)}
        {@const busy = allChecking || $checking.has(id)}
        {@const wait = $setupWaits.get(id)}
        {@const rawStep = entry.enabled && !wait ? nextStep(p) : null}
        {@const step = id === "acp" && rawStep?.label !== "Try again" ? null : rawStep}
        {@const ready = entry.enabled && isUsable(p)}
      <div class="pref-section">
        <div class="rows" class:off={!entry.enabled}>
          <div class="row row-title">
            <ProviderLogo provider={id} size={20} />
            <span class="row-label">{nameOf(id)}{#if id === "acp" && entry.label} · {entry.label}{/if}</span>
            {#if ready && p && !p.gated}
              <span class="chip warn" use:tooltip={"It changes files on its own. Parzi cannot ask you first or keep it inside the project folder."}>Edits without asking</span>
            {/if}
            <span class="row-ctl auto">
              <span class="status-dot {dotState(p, entry.enabled, busy)}" use:tooltip={statusLine(p, entry.enabled, busy)} />
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

          <div class="details">
            {#if id === "acp"}
              <label class="row">
                <span class="row-label">Name</span>
                <input
                  class="tin fixed"
                  placeholder="Custom"
                  aria-label="ACP name"
                  spellcheck="false"
                  bind:value={acpLabel}
                  on:change={saveAcpLabel}
                  on:keydown={(e) => e.key === "Enter" && saveAcpLabel()}
                />
              </label>
            {/if}
            <div class="row">
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
              <span class="row-ctl auto">
                {#if step}
                  <button class="sbtn primary" disabled={busy} on:click={() => step.run(id)}>{busy && step.label === "Try again" ? "Checking…" : step.label}</button>
                {:else if wait}
                  <span class="waiting">{wait.action === "install" ? "Installing…" : "Signing in…"}</span>
                {/if}
                <button class="sbtn onoff" title={entry.enabled ? `Turn ${nameOf(id)} off` : `Turn ${nameOf(id)} on`} on:click={() => setEnabled(id, !entry.enabled)}>{entry.enabled ? "On" : "Off"}</button>
              </span>
            </div>
            {#if id === "acp"}
              <div class="row row-title">
                <span>Custom</span>
                <span class="row-ctl auto">
                  <button class="sbtn" type="button" disabled={registryBusy} on:click={loadRegistry}>
                    {registryBusy ? "Loading…" : "Registry"}
                  </button>
                </span>
              </div>
              {#if registryErr}
                <div class="row"><span class="row-label bad">{registryErr}</span></div>
              {/if}
              {#if registry.length}
                <div class="row">
                  <div class="packs">
                    {#each registry as row (row.id)}
                      <button class="pack-btn" type="button" title={formatCommand(row.command, row.args)} on:click={() => useRegistry(row)}>
                        <span class="pack-name">{row.name}</span>
                        <span class="pack-cmd">{formatCommand(row.command, row.args)}</span>
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}
            {/if}
          </div>
        </div>
      </div>
      {/each}
{/if}

<style>
  .rows.off .row:first-child > :not(.row-ctl) {
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

  /* Status marker: a quiet dot, top right — green when ready. */
  .status-dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--muted);
  }
  .status-dot.ok { background: var(--ok); }
  .status-dot.bad { background: var(--bad); }
  .status-dot.warn { background: var(--warn); }
  .status-dot.off { background: var(--faint); }
  .status-dot.idle { background: transparent; border: 1px solid var(--line); }

  /* Program/Command fields always visible; same 32px inputs as theme cards. */
  .details .tin,
  .details .sbtn {
    height: calc(var(--space) * 8);
  }
  .onoff {
    min-width: calc(var(--space) * 14);
    justify-content: center;
  }
  /* Registry grid uses the same card theme as Appearance packs. */
  .packs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: calc(var(--space) * 3);
    flex: 1;
    min-width: 0;
    width: 100%;
  }
  .pack-btn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space);
    min-width: 0;
    padding: calc(var(--space) * 2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .pack-btn:hover {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
  .pack-name {
    font-size: var(--fs-sm);
    font-weight: 500;
    color: var(--text);
  }
  .pack-cmd {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
</style>
