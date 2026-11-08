<script lang="ts">
  import ProviderLogo from "../ProviderLogo.svelte";
  import { onMount } from "svelte";
  import { api, type ParziConfig, type ProviderEntry, type ProviderStatus, type RegistryAgent } from "../api";
  import { formatCommand, splitCommand } from "../acpCommand";
  import { board, checking, ensureBoard, nextStep, refreshBoard, refreshStale, setupWaits, signInAgent } from "../providerStore";
  import { BILLING_OPTIONS, PROVIDER_ORDER, billingNote, isUsable, nameOf, stateLabel, type Billing } from "../providerRows";
  import { updateConfig } from "../config";
  import { refreshRemoteBoard, remote, remoteBoard, remoteInfo } from "../remote";
  import SegControl from "./SegControl.svelte";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  let cfg: ParziConfig | null = null;
  let loading = true;
  let err = "";
  let paths: Record<string, string> = {};
  let registry: RegistryAgent[] = [];
  let registryErr = "";
  let registryBusy = false;
  let acpLabel = "";
  let savedKeys = new Set<string>();
  let keyDrafts: Record<string, string> = {};
  const AGENT_ORDER = [...PROVIDER_ORDER, "acp"];

  onMount(() => {
    const onFocus = () => {
      if (where === "remote") void checkRemote(true);
    };
    window.addEventListener("focus", onFocus);
    void load();
    return () => window.removeEventListener("focus", onFocus);
  });

  async function load() {
    try {
      const [, c, keys] = await Promise.all([ensureBoard(), api.getConfig(), api.agentKeys()]);
      cfg = c;
      savedKeys = new Set(keys);
      paths = pathsOf(c);
      acpLabel = c.providers?.acp?.label ?? "";
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
    void refreshStale().catch(() => {});
  }

  // The agents on this computer, or on the linked server.
  let where: "here" | "remote" = "here";
  let remoteBusy = false;
  $: places = $remoteInfo ? [{ value: "here", label: "This computer" }, { value: "remote", label: $remoteInfo.label }] : [];
  $: if (!$remoteInfo) where = "here";

  async function checkRemote(refresh: boolean) {
    remoteBusy = true;
    try {
      await refreshRemoteBoard(refresh);
    } catch (e) {
      notify(`Can't reach ${$remoteInfo?.label ?? "the server"}: ${e}`, true);
    } finally {
      remoteBusy = false;
    }
  }

  function pickPlace(v: string) {
    where = v === "remote" ? "remote" : "here";
    if (where === "remote") void checkRemote(!$remoteBoard.length);
  }

  async function remoteAction(id: string, action: "install" | "login") {
    try {
      await remote.agent(id, action);
      notify("Finish in the terminal window");
    } catch (e) {
      notify(String(e), true);
    }
  }

  $: remoteRows = PROVIDER_ORDER.map((id) => ({ id, p: $remoteBoard.find((b) => b.provider === id) }));

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
    if (p.state === "error" || p.state === "signed_out") return p.hint || stateLabel(p);
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

  async function setBilling(id: string, picked: string) {
    const billing = picked as Billing;
    if (billing === (entryOf(id).billing ?? "subscription")) return;
    if (await update(patchEntry(id, { billing }))) void recheck([id]);
  }

  async function setKey(id: string, key: string | null) {
    try {
      await api.setAgentKey(id, key);
      savedKeys = new Set(await api.agentKeys());
      keyDrafts = { ...keyDrafts, [id]: "" };
      notify(key ? "API key saved" : "API key removed");
      void recheck([id]);
    } catch (e) {
      notify(`Could not save: ${e}`, true);
    }
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
  {#if places.length}
    <div class="pref-section">
      <div class="rows">
        <div class="row ctl-row">
          <span class="row-label">Agents on</span>
          <span class="row-ctl auto">
            {#if where === "remote"}
              <button class="btn" disabled={remoteBusy} on:click={() => checkRemote(true)}>{remoteBusy ? "Checking…" : "Check again"}</button>
            {/if}
            <SegControl label="Whose agents" options={places} value={where} on:pick={(e) => pickPlace(e.detail)} />
          </span>
        </div>
      </div>
    </div>
  {/if}
  {#if where === "remote"}
    {#each remoteRows as { id, p } (id)}
      <div class="pref-section">
        <div class="rows">
          <div class="row row-title">
            <ProviderLogo provider={id} size={20} />
            <span>{nameOf(id)}</span>
          </div>
          <div class="row">
            <span class="row-label">{statusLine(p, true, remoteBusy)}</span>
            <span class="row-ctl auto">
              {#if p?.state === "not_installed"}
                <button class="btn primary" on:click={() => remoteAction(id, "install")}>Install</button>
              {:else if p && (p.state === "signed_out" || p.state === "error")}
                <button class="btn primary" on:click={() => remoteAction(id, "login")}>Sign in</button>
              {/if}
            </span>
          </div>
          {#if p?.state === "ready" && p.usage?.length}
            {#each p.usage as w (w.label)}
              <div class="row">
                <span class="usage-label">{w.label}</span>
                <span class="usage-bar"><span class="usage-fill" class:hot={w.used_percent >= 80} style="width:{Math.min(100, w.used_percent)}%" /></span>
                <span class="usage-num">{Math.round(w.used_percent)}%</span>
                {#if w.resets_at}<span class="usage-reset">{resetsIn(w.resets_at)}</span>{/if}
              </div>
            {/each}
          {/if}
        </div>
      </div>
    {/each}
  {:else}
    {#each AGENT_ORDER as id (id)}
        {@const p = statusOf(id)}
        {@const entry = entryOf(id)}
        {@const busy = allChecking || $checking.has(id)}
        {@const wait = $setupWaits.get(id)}
        {@const rawStep = entry.enabled && !wait ? nextStep(p) : null}
        {@const billing = entry.billing ?? "subscription"}
        {@const keyField = billing === "api_key" && id !== "acp" && id !== "opencode"}
        {@const step = (id === "acp" || keyField) && rawStep?.label !== "Try again" && rawStep?.label !== "Install" ? null : rawStep}
        {@const ready = entry.enabled && isUsable(p)}
      <div class="pref-section">
        <div class="rows" class:off={!entry.enabled}>
          <div class="row row-title">
            <ProviderLogo provider={id} size={20} />
            <span>{nameOf(id)}{#if id === "acp" && entry.label} · {entry.label}{/if}</span>
          </div>
          <div class="row">
            <span class="row-label">{wait?.note ?? statusLine(p, entry.enabled, busy)}</span>
            <span class="row-ctl auto">
              {#if step}
                <button class="btn primary" disabled={busy} on:click={() => step.run(id)}>{busy && step.label === "Try again" ? "Checking…" : step.label}</button>
              {:else if wait}
                <span class="waiting">{wait.action === "install" ? "Installing…" : "Signing in…"}</span>
              {:else if ready}
                <button class="btn" title={`Disconnect ${nameOf(id)}`} on:click={() => setEnabled(id, false)}>Connected</button>
              {:else}
                <button class="btn" title={`Connect ${nameOf(id)}`} on:click={() => setEnabled(id, true)}>Disconnected</button>
              {/if}
            </span>
          </div>

          {#if wait}
            {#key wait.note}
              <div class="bar"><span class="progress" class:sweep={!wait.eta} style:--eta="{wait.eta}ms"><span /></span></div>
            {/key}
          {/if}

          {#if id !== "acp" && entry.enabled}
            <div class="row">
              <span class="row-label" class:keyed={billing === "api_key"}>{billingNote(id, billing)}</span>
              <span class="row-ctl auto">
                <SegControl label={`What ${nameOf(id)} bills`} options={BILLING_OPTIONS} value={billing} on:pick={(e) => setBilling(id, e.detail)} />
              </span>
            </div>
            {#if keyField && savedKeys.has(id)}
              <div class="row">
                <span class="row-label">API key saved on this computer</span>
                <span class="row-ctl auto"><button class="btn" on:click={() => setKey(id, null)}>Remove</button></span>
              </div>
            {:else if keyField}
              <div class="row">
                <input
                  class="tin mono key"
                  type="password"
                  placeholder="Paste API key"
                  aria-label={`${nameOf(id)} API key`}
                  autocomplete="off"
                  spellcheck="false"
                  bind:value={keyDrafts[id]}
                  on:keydown={(e) => e.key === "Enter" && keyDrafts[id]?.trim() && setKey(id, keyDrafts[id])}
                />
                <span class="row-ctl auto"><button class="btn primary" disabled={!keyDrafts[id]?.trim()} on:click={() => setKey(id, keyDrafts[id])}>Add key</button></span>
              </div>
            {:else if id === "opencode" && billing === "api_key"}
              <div class="row">
                <span class="row-label">OpenCode keeps a key per provider</span>
                <span class="row-ctl auto"><button class="btn" on:click={() => signInAgent(id)}>Add key</button></span>
              </div>
            {/if}
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

          {#if id === "acp"}
            <div class="details">
              <label class="row prog">
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
              <div class="row prog">
                <span class="row-label">Command</span>
                <input
                  class="tin mono"
                  placeholder="gemini --acp"
                  aria-label="ACP command"
                  spellcheck="false"
                  bind:value={paths[id]}
                  on:change={() => savePath(id)}
                  on:keydown={(e) => e.key === "Enter" && savePath(id)}
                />
              </div>
              <div class="row row-title">
                <span>Custom</span>
                <span class="row-ctl auto">
                  <button class="btn" type="button" disabled={registryBusy} on:click={loadRegistry}>
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
            </div>
          {/if}
        </div>
      </div>
      {/each}
  {/if}
{/if}

<style>
  /* An agent that is off dims its name, not its switch. */
  .rows.off .row:first-child > :not(.row-ctl) {
    opacity: 0.55;
  }
  .waiting {
    color: var(--muted);
    font-size: var(--fs-md);
    white-space: nowrap;
  }
  .row-label.bad {
    color: var(--bad-text);
  }
  .row-label.keyed {
    color: var(--warn);
  }
  .tin.key {
    flex: 1;
    min-width: 0;
  }

  /* ── Setup progress: a hairline strip between rows ── */

  .rows > .bar {
    margin: 0;
  }
  .progress {
    display: block;
    height: 3px;
    overflow: hidden;
    background: var(--line);
  }
  .progress > span {
    display: block;
    width: 100%;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    animation: fill var(--eta) linear forwards;
  }
  .progress.sweep > span {
    width: 30%;
    animation: sweep 1.4s ease-in-out infinite;
  }
  @keyframes fill {
    from {
      transform: scaleX(0);
    }
    to {
      transform: scaleX(1);
    }
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .progress > span,
    .progress.sweep > span {
      animation: none;
    }
  }

  /* ── Usage: label, bar, percent, reset ── */

  .usage-label {
    flex: none;
    width: 96px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .usage-bar {
    flex: 1;
    min-width: 40px;
    height: 5px;
    overflow: hidden;
    background: var(--line);
    border-radius: var(--radius-sm);
  }
  .usage-fill {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: var(--radius-sm);
  }
  .usage-fill.hot {
    background: var(--warn);
  }
  .usage-num {
    flex: none;
    width: 34px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .usage-reset {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-md);
  }

  /* ── Custom agent: program and command fields ── */

  /* One field width, so rows do not bounce; labels take the slack. */
  .rows > .details > .row > input.tin.mono,
  .rows > .details > .row > input.tin.fixed {
    flex: none;
    width: var(--w-field);
    max-width: 100%;
  }
  .rows > .details > div.row.prog,
  .rows > .details > label.row.prog {
    justify-content: flex-start;
  }
</style>
