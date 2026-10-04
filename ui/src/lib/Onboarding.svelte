<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { fade, fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { brain, onboard, type Scan } from "./api";
  import { board, checking, refreshBoard } from "./providerStore";
  import { PROVIDER_ORDER, isUsable, nameOf, stateLabel } from "./providerRows";
  import { importBrowser, onboarded } from "./browserData";
  import { setOverlay } from "./overlay";
  import { toastError } from "./toast";

  const dispatch = createEventDispatcher<{ close: void; openBrain: void }>();

  const STEPS = ["Agents", "Browser", "Your tools", "Brain"];

  let step = 0;
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

  $: agents = PROVIDER_ORDER.map((id) => $board.find((b) => b.provider === id)).filter((b) => !!b);
  $: browsers = scan?.browsers ?? [];
  $: tools = (scan?.tools ?? []).filter((t) => t.found);
  $: folders = (scan?.folders ?? []).filter((f) => f.exists);

  onMount(() => {
    setOverlay("onboarding", true);
    void load();
    return () => setOverlay("onboarding", false);
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

  async function signIn(provider: string) {
    try {
      await onboard.login(provider);
    } catch (e) {
      toastError(e);
    }
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

  function finish(openBrain = false) {
    onboarded.set(true);
    dispatch("close");
    if (openBrain) dispatch("openBrain");
  }

  function ago(ms?: number | null) {
    if (!ms) return "";
    const days = Math.floor((Date.now() - ms) / 86_400_000);
    return days <= 0 ? "today" : days === 1 ? "yesterday" : `${days}d ago`;
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
        <p class="lead">Parzi drives each agent on your own subscription. Sign in with the agent's own login; Parzi never sees your tokens.</p>
        <div class="list">
          {#each agents as a (a.provider)}
            <div class="item">
              <span class="logo">
                {#if hasMark(a.provider)}<ProviderLogo provider={a.provider} size={18} />{:else}{a.provider.slice(0, 1).toUpperCase()}{/if}
              </span>
              <span class="grow">
                <span class="title">{nameOf(a.provider)}</span>
                <span class="sub" class:ok={isUsable(a)}>{a.state === "not_installed" ? a.hint || "Not installed" : stateLabel(a)}</span>
              </span>
              {#if a.state === "signed_out" || a.state === "unchecked"}
                <button class="btn" on:click={() => signIn(a.provider)}>Sign in</button>
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
      {:else}
        <h2>Your brain</h2>
        <p class="lead">
          Plain markdown notes in <code>{vault || "~/.parzi/brain"}</code>. Link them with <code>[[wikilinks]]</code>; any Obsidian install can open the folder as a vault.
        </p>
        <ul class="points">
          <li>A project is a note with a folder. Sessions started in that folder get the project's notes automatically.</li>
          <li>Map a note to a project from the Brain tab, or link it to the project note.</li>
          <li>Agents can search, read and write the brain while they work. Writes ask first unless you allow them.</li>
        </ul>
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
