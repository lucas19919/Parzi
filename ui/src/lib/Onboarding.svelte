<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { get } from "svelte/store";
  import { fade, fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, brain, onboard, onSignIn, type ProviderStatus, type Scan } from "./api";
  import { board, checking, refreshBoard } from "./providerStore";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { importBrowser, onboarded } from "./browserData";
  import { setOverlay } from "./overlay";
  import { mergeProgress, onRemoteProgress, remote, stepName, type RemoteInfo, type RemoteProgress } from "./remote";
  import { toastError } from "./toast";

  const dispatch = createEventDispatcher<{ close: void; openBrain: void; openRemote: void }>();

  const STEPS = ["Agents", "Browser", "Your tools", "Brain", "Remote"];
  const WATCH_EVERY = 5_000;
  const INSTALL_WAIT = 600_000;
  const SIGN_IN_WAIT = 180_000;
  const START_ETA = 75_000;

  interface Wait {
    note: string;
    eta: number;
  }

  export let startStep = 0;

  let step = startStep;
  let scan: Scan | null = null;
  let scanError = "";
  let browserPick = "";
  let wantBookmarks = true;
  let wantHistory = true;
  let browserResult = "";
  let busy = false;
  let notePicks = new Set<string>();
  let folderPicks = new Set<string>();
  let toolResult = "";
  let vault = "";
  let waiting = new Map<string, Wait>();
  let alive = true;

  $: agents = PROVIDER_ORDER.map((id) => $board.find((b) => b.provider === id)).filter((b) => !!b);
  $: browsers = scan?.browsers ?? [];
  $: tools = (scan?.tools ?? []).filter((t) => t.found);
  $: folders = (scan?.folders ?? []).filter((f) => f.exists);

  onMount(() => {
    setOverlay("onboarding", true);
    void load();
    const unlisten = onSignIn(({ provider, step }) => {
      if (!waiting.has(provider)) return;
      waiting = new Map(waiting).set(
        provider,
        step === "starting"
          ? { note: `Starting ${nameOf(provider)}. This takes about a minute…`, eta: START_ETA }
          : { note: "Sign in with Google in your browser…", eta: 0 },
      );
    });
    return () => {
      alive = false;
      void unlisten.then((off) => off());
      setOverlay("onboarding", false);
    };
  });

  async function load() {
    void refreshBoard().catch(() => {});
    brain.dir().then((d) => (vault = d)).catch(() => {});
    try {
      scan = await onboard.scan();
      const first = scan.browsers.find((b) => b.id === "brave") ?? scan.browsers[0];
      browserPick = first ? `${first.id}:${first.profile}` : "";
      notePicks = new Set(scan.tools.flatMap((t) => t.notes.map((n) => n.source)));
      folderPicks = new Set(scan.folders.filter((f) => f.exists).slice(0, 12).map((f) => f.path));
    } catch (e) {
      scanError = String(e);
    }
  }

  function toggle(set: Set<string>, key: string) {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  }

  const stateOf = (provider: string) => get(board).find((b) => b.provider === provider)?.state;

  async function watch(provider: string, from: string | undefined, ms: number) {
    const until = Date.now() + ms;
    while (alive && Date.now() < until) {
      await new Promise((r) => setTimeout(r, WATCH_EVERY));
      const fresh = await refreshBoard([provider]).catch(() => [] as ProviderStatus[]);
      const now = fresh.find((b) => b.provider === provider)?.state;
      if (now && now !== from) return;
    }
  }

  async function act(provider: string, note: string, work: () => Promise<number>) {
    if (waiting.has(provider)) return;
    const from = stateOf(provider);
    waiting = new Map(waiting).set(provider, { note, eta: 0 });
    try {
      const ms = await work();
      if (ms) await watch(provider, from, ms);
      else await refreshBoard([provider]);
    } catch (e) {
      toastError(e);
    } finally {
      const next = new Map(waiting);
      next.delete(provider);
      waiting = next;
    }
  }

  const install = (provider: string) =>
    act(provider, "Installing in the terminal window…", async () => {
      await onboard.install(provider);
      return INSTALL_WAIT;
    });

  const signIn = (provider: string) =>
    act(provider, "Finish signing in in the window that opens…", async () =>
      (await onboard.login(provider)) ? 0 : SIGN_IN_WAIT,
    );

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
    busy = true;
    try {
      const r = await onboard.importTools([...notePicks], [...folderPicks]);
      toolResult = `Added ${r.notes} notes and ${r.projects} projects to your brain.${r.skipped.length ? ` Skipped ${r.skipped.length}.` : ""}`;
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  function finish(openBrain = false, openRemote = false) {
    onboarded.set(true);
    dispatch("close");
    if (openBrain) dispatch("openBrain");
    if (openRemote) dispatch("openRemote");
  }

  function ago(ms?: number | null) {
    if (!ms) return "";
    const days = Math.floor((Date.now() - ms) / 86_400_000);
    return days <= 0 ? "today" : days === 1 ? "yesterday" : `${days}d ago`;
  }

  let remoteInfo: RemoteInfo | null = null;
  let remoteLoaded = false;
  let remoteUser = "";
  let remoteHost = "";
  let remotePass = "";
  let bringNotes = true;
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
  // The password lives in this field until Set up reads it, then only
  // inside the backend call. It is never written anywhere.

  async function loadRemote() {
    try {
      remoteInfo = await remote.info();
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
      remoteInfo = await remote.setup(user, host, password, bringNotes);
      setupState = "done";
      setupMsg = `Linked to ${remoteInfo.label}.`;
    } catch (e) {
      setupState = "error";
      // The failed step row already says why; repeat it only if no row did.
      setupMsg = setupRows.some((r) => r.state === "fail") ? "" : String(e);
    } finally {
      off();
    }
  }

  $: if (step === 4 && !remoteLoaded) {
    remoteLoaded = true;
    void loadRemote();
  }
</script>

<div class="backdrop" transition:fade={{ duration: 150 }} role="presentation">
  <div class="card" role="dialog" aria-modal="true" aria-label="Set up Parzi" transition:fly={{ y: 10, duration: 180 }}>
    <header>
      <div class="steps">
        {#each STEPS as s, i}
          <button class:on={i === step} class:done={i < step} on:click={() => (step = i)}>{s}</button>
        {/each}
      </div>
      <button class="x" title="Close" on:click={() => finish()}><Icon name="close" size={13} stroke={2} /></button>
    </header>

    <div class="body">
      {#if step === 0}
        <h2>Your agents</h2>
        <p class="lead">Parzi drives each agent on your own subscription. Install and sign in right here; each agent keeps its own login, so Parzi never sees your tokens.</p>
        <div class="list">
          {#each agents as a (a.provider)}
            {@const wait = waiting.get(a.provider)}
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
              {#if a.state === "not_installed"}
                <button class="btn" disabled={waiting.has(a.provider)} on:click={() => install(a.provider)}>
                  {waiting.has(a.provider) ? "Installing…" : "Install"}
                </button>
              {:else if a.state === "signed_out" || a.state === "unchecked"}
                <button class="btn" disabled={waiting.has(a.provider)} on:click={() => signIn(a.provider)}>
                  {waiting.has(a.provider) ? "Signing in…" : "Sign in"}
                </button>
              {/if}
            </div>
          {/each}
        </div>
        <button class="link" disabled={$checking.size > 0} on:click={() => refreshBoard().catch(() => {})}>
          {$checking.size ? "Checking…" : "Check again"}
        </button>
      {:else if step === 1}
        <h2>Bring your browser</h2>
        <p class="lead">Bookmarks and history only. Passwords and cookies stay where they are.</p>
        {#if !browsers.length}
          <p class="empty">{scanError || (scan ? "No Brave, Chrome or Edge profile found." : "Looking…")}</p>
        {:else}
          <div class="list">
            {#each browsers as b (b.id + b.profile)}
              <label class="item" class:on={browserPick === `${b.id}:${b.profile}`}>
                <input type="radio" bind:group={browserPick} value={`${b.id}:${b.profile}`} />
                <span class="grow">
                  <span class="title">{b.name}</span>
                  <span class="sub">{b.profile} · {b.bookmarks} bookmarks{b.history ? " · history" : ""}</span>
                </span>
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
        <h2>Bring your other tools</h2>
        <p class="lead">Instructions, memories and skills from your other agents become notes in your brain. Folders you've worked in become projects.</p>
        {#if !scan}
          <p class="empty">{scanError || "Looking…"}</p>
        {:else}
          {#if tools.length}
            <h3>Notes</h3>
            <div class="list scroll">
              {#each tools as t (t.id)}
                {#each t.notes as n (n.source)}
                  <label class="item compact">
                    <input type="checkbox" checked={notePicks.has(n.source)} on:change={() => (notePicks = toggle(notePicks, n.source))} />
                    <span class="grow">
                      <span class="title">{n.title}</span>
                      <span class="sub">{t.name} · {n.kind}</span>
                    </span>
                  </label>
                {:else}
                  <div class="item compact"><span class="grow"><span class="title">{t.name}</span><span class="sub">found, nothing to import</span></span></div>
                {/each}
              {/each}
            </div>
          {/if}
          {#if folders.length}
            <h3>Projects</h3>
            <div class="list scroll">
              {#each folders as f (f.path)}
                <label class="item compact">
                  <input type="checkbox" checked={folderPicks.has(f.path)} on:change={() => (folderPicks = toggle(folderPicks, f.path))} />
                  <span class="grow">
                    <span class="title">{f.name}</span>
                    <span class="sub">{f.path}{f.last_used ? ` · ${ago(f.last_used)}` : ""}</span>
                  </span>
                </label>
              {/each}
            </div>
          {/if}
          <div class="action">
            <button class="btn primary" disabled={busy || (!notePicks.size && !folderPicks.size)} on:click={importFromTools}>
              {busy ? "Importing…" : `Import ${notePicks.size} notes, ${folderPicks.size} projects`}
            </button>
            {#if toolResult}<span class="result">{toolResult}</span>{/if}
          </div>
        {/if}
      {:else if step === 3}
        <h2>Your brain</h2>
        <p class="lead">
          Plain markdown notes in <code>{vault || "~/.parzi/brain"}</code>. Link them with <code>[[wikilinks]]</code>; any Obsidian install can open the folder as a vault.
        </p>
        <ul class="points">
          <li>A project is a note with a folder. Sessions started in that folder get the project's notes automatically.</li>
          <li>Map a note to a project from the Brain tab, or link it to the project note.</li>
          <li>Agents can search, read and write the brain while they work. Writes ask first unless you allow them.</li>
        </ul>
      {:else}
        <h2>Work from anywhere</h2>
        <p class="lead">
          Run Parzi on your own Linux server so sessions keep going while this PC sleeps. Enter how you reach it over SSH. Parzi installs itself there, starts its engine and links back.
        </p>
        {#if remoteInfo && setupState !== "running"}
          <div class="item">
            <span class="grow">
              <span class="title">{remoteInfo.label}</span>
              <span class="sub ok">Linked{remoteInfo.version ? ` · Parzi ${remoteInfo.version}` : ""}</span>
            </span>
            <button class="btn" on:click={() => finish(false, true)}>Open</button>
          </div>
        {/if}
        <div class="remote-form">
          <label>
            <span>User</span>
            <input placeholder="you" spellcheck="false" autocomplete="username" bind:value={remoteUser} />
          </label>
          <label>
            <span>Host</span>
            <input placeholder="gpu-box or 203.0.113.7" title="host, or host:port" spellcheck="false" bind:value={remoteHost} />
          </label>
          <label>
            <span>Password <em>(optional)</em></span>
            <input type="password" placeholder="only if there is no key login" autocomplete="current-password" bind:value={remotePass} />
          </label>
        </div>
        <label class="check">
          <input type="checkbox" bind:checked={bringNotes} />
          <span>Bring my brain notes (project notes stay here)</span>
        </label>
        <div class="action">
          <button
            class="btn primary"
            disabled={!remoteUser.trim() || !remoteHost.trim() || setupState === "running"}
            on:click={setupRemote}
          >
            {setupState === "running" ? "Setting up…" : remoteInfo ? "Set up again" : "Set up"}
          </button>
          {#if setupMsg}<span class="result" class:bad={setupState === "error"}>{setupMsg}</span>{/if}
        </div>
        {#if setupRows.length}
          <ul class="setup-log">
            {#each setupRows as r (r.step)}
              <li>
                <span class="mark {r.state}">
                  {#if r.state === "ok"}<Icon name="check" size={13} />{:else if r.state === "fail"}<Icon name="close" size={13} />{:else if r.state === "run"}<span class="spin" />{/if}
                </span>
                <span class="name">{stepName(r.step)}</span>
                <span class="detail">{r.detail}</span>
              </li>
            {/each}
          </ul>
        {/if}
        <p class="fine">With a password, Parzi adds its own SSH key to that account so it can reconnect without one. The password itself is never stored.</p>
      {/if}
    </div>

    <footer>
      {#if step > 0}<button class="btn" on:click={() => (step -= 1)}>Back</button>{/if}
      <span class="grow" />
      {#if step < STEPS.length - 1}
        <button class="link" on:click={() => (step += 1)}>Skip</button>
        <button class="btn primary" on:click={() => (step += 1)}>Next</button>
      {:else}
        <button class="btn" on:click={() => finish()}>Done</button>
        <button class="btn primary" on:click={() => finish(true)}>Open brain</button>
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
    padding: 24px;
    background: rgba(0, 0, 0, 0.6);
  }
  .card {
    width: min(620px, 100%);
    max-height: min(720px, 100%);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header,
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
  }
  header {
    border-bottom: 1px solid var(--line);
  }
  footer {
    border-top: 1px solid var(--line);
  }
  .steps {
    display: flex;
    gap: 4px;
    flex: 1;
  }
  .steps button {
    padding: 4px 10px;
    background: none;
    border: none;
    border-radius: 999px;
    color: var(--faint);
    font-size: 12px;
    cursor: pointer;
  }
  .steps button.done {
    color: var(--muted);
  }
  .steps button:hover {
    color: var(--text);
  }
  .steps button.on {
    background: var(--line);
    color: var(--text);
  }
  .x {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    background: var(--line);
    color: var(--text);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 20px 22px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
    color: var(--text);
  }
  h3 {
    margin: 16px 0 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .lead {
    margin: 0 0 16px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--muted);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .list.scroll {
    max-height: 190px;
    overflow-y: auto;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: default;
    transition: background 140ms ease, border-color 140ms ease;
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
    padding: 5px 10px;
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
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .grow {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text);
  }
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    color: var(--faint);
  }
  .sub.ok {
    color: var(--ok);
  }
  .sub.mono {
    font-family: var(--mono), ui-monospace, monospace;
    color: var(--muted);
  }
  .remote-form {
    display: flex;
    gap: 8px;
    margin: 4px 0 8px;
  }
  .remote-form label {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    color: var(--faint);
  }
  .remote-form input {
    padding: 7px 10px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: 12.5px;
    outline: none;
  }
  .remote-form input:focus {
    border-color: var(--accent);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 13px;
    color: var(--muted);
    cursor: pointer;
  }
  .result.bad {
    color: var(--bad);
  }
  .setup-log {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .setup-log li {
    display: grid;
    grid-template-columns: 18px 130px 1fr;
    align-items: baseline;
    gap: 6px;
    font-size: 12.5px;
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
    color: var(--ok);
  }
  .mark.fail {
    color: var(--bad);
  }
  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 999px;
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
    margin: 12px 0 0;
    font-size: 12px;
    color: var(--faint);
  }
  .progress {
    position: absolute;
    left: 48px;
    right: 10px;
    bottom: 3px;
    height: 2px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--line);
  }
  .progress span {
    position: absolute;
    inset: 0 8% 0 0;
    border-radius: inherit;
    background: var(--accent);
    transform-origin: left;
    animation: fill var(--eta) cubic-bezier(0.2, 0.6, 0.3, 1) forwards;
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
    gap: 18px;
    margin: 12px 10px 0;
    font-size: 13px;
    color: var(--muted);
  }
  .checks label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }
  .action {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 16px;
  }
  .action .btn {
    flex: none;
    white-space: nowrap;
  }
  .result {
    font-size: 12px;
    color: var(--ok);
  }
  .empty {
    font-size: 13px;
    color: var(--faint);
  }
  .points {
    margin: 0;
    padding-left: 18px;
    font-size: 13px;
    line-height: 1.6;
    color: var(--muted);
  }
  code {
    font-size: 12px;
    padding: 1px 5px;
    background: var(--bg);
    border-radius: 4px;
  }
  .link {
    padding: 4px 8px;
    background: none;
    border: none;
    color: var(--muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .link:hover:not(:disabled) {
    color: var(--text);
  }
</style>
