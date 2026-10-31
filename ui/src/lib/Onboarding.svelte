<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { fade, fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { brain, onboard, type Scan } from "./api";
  import { board, checking, nextStep, refreshBoard, setupWaits } from "./providerStore";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { importBrowser, onboarded } from "./browserData";
  import { setOverlay } from "./overlay";
  import { mergeProgress, onRemoteProgress, remote, remoteInfo as linked, stepName, type RemoteInfo, type RemoteProgress } from "./remote";
  import { toastError } from "./toast";
  import { tooltip } from "./tooltip";

  const dispatch = createEventDispatcher<{ close: void }>();

  const STEPS = ["Agents", "Browser", "Your tools", "Done"];
  const TITLES = ["Your agents", "Bring your browser", "Bring your other tools", "You're set"];
  const LAST = STEPS.length - 1;
  const RECENT = 12;
  export let startStep = 0;
  export let server = false;

  let step = startStep;
  let scan: Scan | null = null;
  let scanError = "";
  let browserPick = "";
  let wantBookmarks = true;
  let wantHistory = true;
  let browserResult = "";
  let busy = false;
  let picked = new Set<string>();
  let open = new Set<string>();
  let toolResult = "";
  let skipped: string[] = [];
  let vault = "";
  let serverOpen = server;
  let card: HTMLDivElement;

  type Group = { id: string; name: string; unit: [string, string]; items: { key: string; title: string; hint: string }[] };

  $: agents = PROVIDER_ORDER.map((id) => $board.find((b) => b.provider === id)).filter((b) => !!b);
  $: browsers = scan?.browsers ?? [];
  $: groups = groupsOf(scan);

  function groupsOf(s: Scan | null): Group[] {
    if (!s) return [];
    const tools: Group[] = s.tools
      .filter((t) => t.found && t.notes.length)
      .map((t) => ({ id: t.id, name: t.name, unit: ["note", "notes"], items: t.notes.map((n) => ({ key: n.source, title: n.title, hint: n.source })) }));
    const folders = s.folders.filter((f) => f.exists).slice(0, RECENT);
    if (!folders.length) return tools;
    return [...tools, { id: "projects", name: "Projects", unit: ["project", "projects"], items: folders.map((f) => ({ key: f.path, title: f.name, hint: f.path })) }];
  }

  onMount(() => {
    setOverlay("onboarding", true);
    const before = document.activeElement as HTMLElement | null;
    card.focus();
    void load();
    return () => {
      setOverlay("onboarding", false);
      before?.focus?.();
    };
  });

  async function load() {
    void refreshBoard().catch(() => {});
    brain.dir().then((d) => (vault = d)).catch(() => {});
    try {
      scan = await onboard.scan();
      const first = scan.browsers.find((b) => b.id === "brave") ?? scan.browsers[0];
      browserPick = first ? `${first.id}:${first.profile}` : "";
      picked = new Set(groupsOf(scan).flatMap((g) => g.items.map((i) => i.key)));
    } catch (e) {
      scanError = String(e);
    }
  }

  const pickedIn = (g: Group, set: Set<string>) => g.items.filter((i) => set.has(i.key)).length;

  function count(g: Group, set: Set<string>) {
    const n = g.items.length;
    const k = pickedIn(g, set);
    const unit = g.unit[n === 1 ? 0 : 1];
    return k && k < n ? `${k} of ${n} ${unit}` : `${n} ${unit}`;
  }

  function pickGroup(g: Group) {
    const all = pickedIn(g, picked) === g.items.length;
    const next = new Set(picked);
    for (const i of g.items) {
      if (all) next.delete(i.key);
      else next.add(i.key);
    }
    picked = next;
  }

  function toggle(set: Set<string>, key: string) {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  }

  function mixed(node: HTMLInputElement, on: boolean) {
    node.indeterminate = on;
    return { update: (v: boolean) => (node.indeterminate = v) };
  }

  async function importFromBrowser() {
    if (!browserPick) return;
    const [id, profile] = browserPick.split(":");
    busy = true;
    try {
      const data = await onboard.browser(id, profile);
      const got = importBrowser({
        bookmarks: wantBookmarks ? data.bookmarks : [],
        history: wantHistory ? data.history : [],
      });
      browserResult = `Imported ${got.bookmarks} bookmarks and ${got.history} history entries.`;
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  async function importFromTools() {
    const keys = (projects: boolean) =>
      groups.filter((g) => (g.id === "projects") === projects).flatMap((g) => g.items.map((i) => i.key).filter((k) => picked.has(k)));
    busy = true;
    try {
      const r = await onboard.importTools(keys(false), keys(true));
      toolResult = `Added ${r.notes} ${r.notes === 1 ? "note" : "notes"} and ${r.projects} ${r.projects === 1 ? "project" : "projects"} to your brain.`;
      skipped = r.skipped;
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  function finish() {
    if (locked) return;
    onboarded.set(true);
    dispatch("close");
  }

  function onKey(e: KeyboardEvent) {
    const typing = (e.target as HTMLElement | null)?.closest?.("input, textarea, select");
    if (e.key !== "Escape" || e.defaultPrevented || typing) return;
    e.preventDefault();
    finish();
  }

  async function next() {
    if (busy) return;
    if (step === 1 && !browserResult && browserPick && (wantBookmarks || wantHistory)) await importFromBrowser();
    else if (step === 2 && !toolResult && picked.size) await importFromTools();
    step += 1;
  }

  let remoteInfo: RemoteInfo | null = null;
  let remoteLoaded = false;
  let remoteUser = "";
  let remoteHost = "";
  let remotePass = "";
  let setupRows: RemoteProgress[] = [];
  let setupState: "idle" | "running" | "done" | "error" = "idle";
  let setupMsg = "";
  try {
    remoteUser = localStorage.getItem("parzi.remote.user") ?? "";
    remoteHost = localStorage.getItem("parzi.remote.host") ?? "";
  } catch {}
  $: try {
    localStorage.setItem("parzi.remote.user", remoteUser);
    localStorage.setItem("parzi.remote.host", remoteHost);
  } catch {}

  $: locked = setupState === "running";

  async function loadRemote() {
    try {
      remoteInfo = await remote.info();
      if (remoteInfo) serverOpen = true;
    } catch (e) {
      toastError(e);
    }
  }

  async function setupRemote() {
    const user = remoteUser.trim();
    const host = remoteHost.trim();
    if (!user || !host || setupState === "running") return;
    setupState = "running";
    setupRows = [];
    setupMsg = "";
    const password = remotePass;
    remotePass = "";
    const off = await onRemoteProgress((p) => (setupRows = mergeProgress(setupRows, p)));
    try {
      remoteInfo = await remote.setup(user, host, password, false);
      linked.set(remoteInfo);
      setupState = "done";
      setupMsg = `Linked to ${remoteInfo.label}.`;
    } catch (e) {
      setupState = "error";
      setupMsg = setupRows.some((r) => r.state === "fail") ? "" : String(e);
    } finally {
      off();
    }
  }

  $: if (step === LAST && !remoteLoaded) {
    remoteLoaded = true;
    void loadRemote();
  }
</script>

<svelte:window on:keydown={onKey} />

<div class="backdrop" transition:fade={{ duration: 150 }} role="presentation">
  <div
    class="card"
    role="dialog"
    aria-modal="true"
    aria-label={TITLES[step]}
    tabindex="-1"
    bind:this={card}
    transition:fly={{ y: 10, duration: 180 }}
  >
    <header class="page-bar">
      <div class="page-tabs" role="tablist" aria-label="Setup">
        {#each STEPS as s, i}
          <button role="tab" aria-selected={i === step} disabled={locked} on:click={() => (step = i)}>{s}</button>
        {/each}
      </div>
      <button class="icon-btn" use:tooltip={locked ? "Wait for the remote setup" : "Back"} disabled={locked} on:click={() => finish()}><Icon name="arrowLeft" /></button>
    </header>

    <div class="body">
      {#if step === 0}
        <p class="lead">Parzi drives each agent on your own subscription. Install and sign in right here; each agent keeps its own login, so Parzi never sees your tokens.</p>
        <div class="list">
          {#each agents as a (a.provider)}
            {@const wait = $setupWaits.get(a.provider)}
            {@const next = wait ? null : nextStep(a)}
            <div class="item">
              <span class="logo">
                {#if hasMark(a.provider)}<ProviderLogo provider={a.provider} size={18} />{:else}{a.provider.slice(0, 1).toUpperCase()}{/if}
              </span>
              <span class="grow">
                <span class="title">{nameOf(a.provider)}</span>
                <span class="sub" class:ok={!wait && isUsable(a)}>{wait?.note ?? stateLabel(a)}</span>
              </span>
              {#if wait}
                {#key wait.note}
                  <span class="progress" class:sweep={!wait.eta} style:--eta="{wait.eta}ms"><span></span></span>
                {/key}
              {/if}
              {#if next}
                <button class="btn" disabled={$checking.has(a.provider) || $checking.has("*")} on:click={() => next.run(a.provider)}>{next.label}</button>
              {/if}
            </div>
          {/each}
        </div>
        <button class="link" disabled={$checking.size > 0} on:click={() => refreshBoard().catch(() => {})}>
          {$checking.size ? "Checking…" : "Check again"}
        </button>
      {:else if step === 1}
        <p class="lead">Bookmarks and history only. Passwords and cookies stay where they are.</p>
        {#if !browsers.length}
          <p class="empty">{scanError || (scan ? "No Brave, Chrome or Edge profile found." : "Looking…")}</p>
        {:else}
          <div class="list">
            {#each browsers as b (b.id + b.profile)}
              <label class="item" class:on={browserPick === `${b.id}:${b.profile}`}>
                <input type="radio" bind:group={browserPick} value={`${b.id}:${b.profile}`} />
                <span class="line">
                  <span class="title">{b.label}</span>
                  <span class="muted">{b.name}</span>
                </span>
                <span class="count">{b.bookmarks} {b.bookmarks === 1 ? "bookmark" : "bookmarks"}</span>
              </label>
            {/each}
          </div>
          <div class="checks">
            <label><input type="checkbox" bind:checked={wantBookmarks} /> Bookmarks</label>
            <label><input type="checkbox" bind:checked={wantHistory} /> History</label>
          </div>
          <div class="action">
            <button class="btn primary" disabled={busy || !browserPick || (!wantBookmarks && !wantHistory)} on:click={importFromBrowser}>
              {busy ? "Importing…" : "Import"}
            </button>
            {#if browserResult}<span class="result">{browserResult}</span>{/if}
          </div>
        {/if}
      {:else if step === 2}
        <p class="lead">Instructions, memories and skills from your other agents become notes in your brain. Folders you've worked in become projects.</p>
        {#if !scan}
          <p class="empty">{scanError || "Looking…"}</p>
        {:else if !groups.length}
          <p class="empty">Nothing to bring over from other agents.</p>
        {:else}
          <div class="list">
            {#each groups as g (g.id)}
              {@const k = pickedIn(g, picked)}
              <div class="item">
                <label class="pick">
                  <input type="checkbox" checked={k === g.items.length} use:mixed={k > 0 && k < g.items.length} on:change={() => pickGroup(g)} />
                  <span class="title">{g.name}</span>
                </label>
                <button class="more" aria-expanded={open.has(g.id)} on:click={() => (open = toggle(open, g.id))}>
                  {count(g, picked)}
                  <Icon name={open.has(g.id) ? "chevDown" : "chevRight"} size={12} />
                </button>
              </div>
              {#if open.has(g.id)}
                <div class="list picks">
                  {#each g.items as it (it.key)}
                    <label class="item compact" title={it.hint}>
                      <input type="checkbox" checked={picked.has(it.key)} on:change={() => (picked = toggle(picked, it.key))} />
                      <span class="title">{it.title}</span>
                    </label>
                  {/each}
                </div>
              {/if}
            {/each}
          </div>
          <div class="action">
            <button class="btn primary" disabled={busy || !picked.size} on:click={importFromTools}>{busy ? "Importing…" : "Import"}</button>
            {#if toolResult}<span class="result">{toolResult}</span>{/if}
          </div>
          {#if skipped.length}
            <ul class="skipped">
              {#each skipped.slice(0, 5) as s}<li>Skipped {s}</li>{/each}
              {#if skipped.length > 5}<li>Skipped {skipped.length - 5} more</li>{/if}
            </ul>
          {/if}
        {/if}
      {:else}
        <p class="lead">Your project notes are plain markdown in <code>{vault || "~/.parzi/brain"}</code>, so any Obsidian install can open them too.</p>
        <button class="disclose" aria-expanded={serverOpen} disabled={locked} on:click={() => (serverOpen = !serverOpen)}>
          <Icon name={serverOpen ? "chevDown" : "chevRight"} size={12} />
          I have a Linux server
        </button>
        {#if serverOpen}
          <div class="server">
            <p class="lead">Parzi installs itself there over SSH and becomes the home of your projects, settings and sessions. This PC stays in step with it.</p>
            {#if remoteInfo && setupState !== "running"}
              <div class="item">
                <span class="grow">
                  <span class="title">{remoteInfo.label}</span>
                  <span class="sub ok">Linked{remoteInfo.version ? ` · Parzi ${remoteInfo.version}` : ""}. Turn on Remote in the composer to run a session there.</span>
                </span>
              </div>
            {/if}
            <div class="remote-form">
              <label>
                <span>User</span>
                <input placeholder="you" spellcheck="false" autocomplete="username" disabled={locked} bind:value={remoteUser} />
              </label>
              <label>
                <span>Host</span>
                <input placeholder="gpu-box or 203.0.113.7" title="host, or host:port" spellcheck="false" disabled={locked} bind:value={remoteHost} />
              </label>
              <label>
                <span>Password <em>(optional)</em></span>
                <input type="password" placeholder="Only if there is no key login" autocomplete="current-password" disabled={locked} bind:value={remotePass} />
              </label>
            </div>
            <div class="action">
              <button class="btn primary" disabled={!remoteUser.trim() || !remoteHost.trim() || locked} on:click={setupRemote}>
                {locked ? "Setting up…" : remoteInfo ? "Set up again" : "Set up"}
              </button>
              {#if setupMsg}<span class="result" class:bad={setupState === "error"}>{setupMsg}</span>{/if}
            </div>
            {#if setupRows.length}
              <ul class="setup-log">
                {#each setupRows as r (r.step)}
                  <li>
                    <span class="mark {r.state}">
                      {#if r.state === "ok"}<Icon name="check" />{:else if r.state === "fail"}<Icon name="close" />{:else if r.state === "run"}<span class="spin" />{/if}
                    </span>
                    <span class="name">{stepName(r.step)}</span>
                    <span class="detail">{r.detail}</span>
                  </li>
                {/each}
              </ul>
            {/if}
            <p class="fine">With a password, Parzi adds its own SSH key to that account so it can reconnect without one. The password itself is never stored.</p>
          </div>
        {/if}
      {/if}
    </div>

    <footer>
      {#if step > 0}<button class="btn" disabled={locked} on:click={() => (step -= 1)}>Back</button>{/if}
      <span class="grow" />
      {#if step < LAST}
        <button class="link" on:click={() => (step += 1)}>Skip</button>
        <button class="btn primary" disabled={busy} on:click={next}>{busy ? "Importing…" : "Next"}</button>
      {:else}
        <button class="btn primary" disabled={locked} on:click={() => finish()}>Done</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--space) * 6);
    background: var(--scrim);
  }
  .card {
    width: min(640px, calc(100vw - 48px));
    height: min(640px, calc(100vh - 48px));
    min-width: 0;
    min-height: 0;
    max-width: min(640px, calc(100vw - 48px));
    max-height: min(640px, calc(100vh - 48px));
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    outline: none;
  }
  header .page-tabs {
    flex: 1;
  }
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 2) calc(var(--space) * 4);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: calc(var(--space) * 6);
  }
  .lead {
    margin: 0 0 calc(var(--space) * 4);
    font-size: var(--fs-md);
    line-height: 1.5;
    color: var(--muted);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .list.picks {
    max-height: 220px;
    margin-left: calc(var(--space) * 6);
    overflow-y: auto;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: default;
    transition: background var(--dur) ease, border-color var(--dur) ease;
  }
  .item.on {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  label.item {
    cursor: pointer;
  }
  .item:hover {
    background: var(--line);
  }
  .item.compact {
    padding: var(--space) calc(var(--space) * 3);
  }
  .logo {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .grow {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .sub.ok {
    color: var(--ok-text);
  }
  .line {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: calc(var(--space) * 2);
  }
  .muted {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .count {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    cursor: pointer;
  }
  .more,
  .disclose {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 24px;
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .more:hover,
  .disclose:hover:not(:disabled) {
    background: var(--line);
    color: var(--text);
  }
  .disclose {
    margin-left: calc(var(--space) * -2);
    font-size: var(--fs-md);
  }
  .server {
    margin-top: calc(var(--space) * 3);
  }
  .skipped {
    list-style: none;
    margin: calc(var(--space) * 2) 0 0;
    padding: 0;
    font-size: var(--fs-sm);
    line-height: 1.6;
    color: var(--muted);
  }
  .remote-form {
    display: flex;
    gap: calc(var(--space) * 2);
    margin: var(--space) 0 calc(var(--space) * 2);
  }
  .remote-form label {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .remote-form input {
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  .remote-form input:focus {
    border-color: var(--accent);
  }
  .result.bad {
    color: var(--bad-text);
  }
  .setup-log {
    list-style: none;
    margin: calc(var(--space) * 3) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .setup-log li {
    display: grid;
    grid-template-columns: 18px 130px 1fr;
    align-items: baseline;
    gap: calc(var(--space) * 2);
    font-size: var(--fs-md);
  }
  .setup-log .name {
    color: var(--text);
  }
  .setup-log .detail {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .mark {
    display: inline-flex;
    align-self: center;
  }
  .mark.ok {
    color: var(--ok-text);
  }
  .mark.fail {
    color: var(--bad-text);
  }
  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: turn 0.8s linear infinite;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
  .fine {
    margin: calc(var(--space) * 3) 0 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .progress {
    position: absolute;
    left: 48px;
    right: 10px;
    bottom: 3px;
    height: 2px;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--line);
  }
  .progress span {
    position: absolute;
    inset: 0 8% 0 0;
    border-radius: inherit;
    background: var(--accent);
    transform-origin: left;
    animation: fill var(--eta) var(--ease-out) forwards;
  }
  .progress.sweep span {
    inset: 0 auto 0 0;
    width: 30%;
    animation: sweep 1.3s ease-in-out infinite;
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
  .checks {
    display: flex;
    gap: calc(var(--space) * 4);
    margin: calc(var(--space) * 3) calc(var(--space) * 3) 0;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .checks label {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    cursor: pointer;
  }
  .action {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    margin-top: calc(var(--space) * 4);
  }
  .action .btn {
    flex: none;
    white-space: nowrap;
  }
  .result {
    font-size: var(--fs-sm);
    color: var(--ok-text);
  }
  .empty {
    font-size: var(--fs-md);
    color: var(--muted);
  }
  code {
    font-size: var(--fs-sm);
    padding: var(--space) var(--space);
    background: var(--bg);
    border-radius: var(--radius-sm);
  }
  .link {
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: none;
    color: var(--muted);
    font-size: var(--fs-md);
    cursor: pointer;
  }
  .link:hover:not(:disabled) {
    color: var(--text);
  }
</style>
