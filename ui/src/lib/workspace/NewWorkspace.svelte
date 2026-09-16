<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import {
    hub, onCloneEvent,
    type GithubOrg, type GithubRepo, type GithubStatus, type RepoRef, type WorkspaceKind,
  } from "../api";
  import { initFixture } from "../fixtures";
  import GithubConnect from "./GithubConnect.svelte";
  import RepoPicker, { type PickItem } from "./RepoPicker.svelte";

  /** Screenshot aid: `PARZI_UI_STATE=new-workspace:<step>` opens here. */
  export let initialStep = "";

  const dispatch = createEventDispatcher<{ cancel: void; created: { name: string } }>();

  type Step = "kind" | "name" | "github" | "orgs" | "repos" | "init";
  const STEPS: { id: Step; label: string }[] = [
    { id: "kind", label: "Kind" },
    { id: "name", label: "Name" },
    { id: "github", label: "GitHub" },
    { id: "orgs", label: "Orgs" },
    { id: "repos", label: "Repos" },
    { id: "init", label: "Initialise" },
  ];

  let step: Step = "kind";
  let kind: WorkspaceKind = "solo";
  let name = "";
  let gh: GithubStatus | null = null;
  let orgs: GithubOrg[] = [];
  let pickedOrgs: string[] = [];
  let repos: GithubRepo[] = [];
  let pickedRepos: string[] = [];
  let loadingOrgs = false;
  let loadingRepos = false;
  let err = "";

  /** Per-repo clone/map line on the last step. */
  let progress: Record<string, { phase: string; line: string }> = {};
  let initialising = false;
  let done = false;
  /** The repos this run picked, kept so Retry can run the failed ones again. */
  let refs: RepoRef[] = [];
  /** `workspace.toml` is written: the wizard may be left without cloning. */
  let created = false;
  let unlisten: (() => void) | null = null;

  onMount(() => {
    const want = initialStep.trim();
    // Screenshot only: `new-workspace:init` / `:init-failed` draw a clone map
    // that never ran. `initialStep` is empty on every real run.
    const fx = initFixture(want);
    if (fx) {
      step = "init";
      name = fx.name;
      refs = fx.refs;
      progress = fx.progress;
      done = fx.done;
      created = true;
    } else if (want && STEPS.some((s) => s.id === want)) {
      step = want as Step;
    }
    void onCloneEvent((e) => {
      if (e.workspace !== name.trim()) return;
      progress = { ...progress, [e.repo]: { phase: e.phase, line: e.line || e.path } };
      settle();
    }).then((f) => (unlisten = f));
  });

  /** Nothing is still cloning: the step is either ready or showing failures. */
  function settle() {
    const all = Object.values(progress);
    if (all.some((p) => p.phase === "cloning")) return;
    initialising = false;
    done = all.every((p) => p.phase === "done");
  }

  onDestroy(() => unlisten?.());

  $: idx = STEPS.findIndex((s) => s.id === step);
  $: orgItems = [
    { key: "", name: "Your repos", sub: gh?.login ?? "personal" },
    ...orgs.map((o) => ({ key: o.login, name: o.name || o.login, sub: o.login })),
  ] as PickItem[];
  $: repoItems = repos.map((r) => ({
    key: r.full_name,
    name: r.name,
    sub: r.full_name,
    badge: r.private ? "private" : "",
  })) as PickItem[];

  /** Repos whose clone or map came back with an error, and their refs. */
  $: failed = Object.entries(progress)
    .filter(([, p]) => p.phase === "error")
    .map(([repo]) => repo);
  $: failedRefs = refs.filter((r) => failed.includes(r.name));
  /** Nothing is running and the step cannot finish: offer a way out. */
  $: stuck = !initialising && !done && (failed.length > 0 || err !== "");

  $: canNext =
    step === "kind" ||
    (step === "name" && !!name.trim()) ||
    (step === "github" && !!gh?.connected) ||
    (step === "orgs" && pickedOrgs.length > 0) ||
    (step === "repos" && pickedRepos.length > 0);

  async function next() {
    if (!canNext || initialising) return;
    err = "";
    if (step === "kind") { step = "name"; return; }
    if (step === "name") { step = "github"; return; }
    if (step === "github") { step = "orgs"; void loadOrgs(); return; }
    if (step === "orgs") { step = "repos"; void loadRepos(); return; }
    if (step === "repos") { step = "init"; void initialise(); }
  }

  function back() {
    if (idx > 0) step = STEPS[idx - 1].id;
  }

  async function loadOrgs() {
    if (orgs.length || loadingOrgs) return;
    loadingOrgs = true;
    try {
      orgs = await hub.orgs();
    } catch (e) {
      err = String(e);
    } finally {
      loadingOrgs = false;
    }
  }

  async function loadRepos() {
    loadingRepos = true;
    err = "";
    try {
      const batches = await Promise.all(pickedOrgs.map((o) => hub.repos(o)));
      const seen = new Set<string>();
      repos = batches.flat().filter((r) => !seen.has(r.full_name) && seen.add(r.full_name));
    } catch (e) {
      err = String(e);
    } finally {
      loadingRepos = false;
    }
  }

  /** Create `workspace.toml`, record the repos, then clone or map each one. */
  async function initialise() {
    const ws = name.trim();
    initialising = true;
    done = false;
    err = "";
    progress = {};
    refs = repos
      .filter((r) => pickedRepos.includes(r.full_name))
      .map((r) => ({
        name: r.name,
        remote: r.clone_url,
        default_branch: r.default_branch,
        local_path: null,
      }));
    try {
      // Only once: a second pass after a failure must not trip over its own
      // workspace.
      if (!created) {
        await hub.createWorkspace(ws, kind);
        created = true;
      }
      await hub.addRepos(ws, refs);
    } catch (e) {
      err = String(e);
      initialising = false;
      return;
    }
    await runClones(refs);
  }

  /** The whole setup when it never got past `workspace.toml`, the failed
   *  clones when it did. */
  function retry() {
    void (created && !err && failedRefs.length ? runClones(failedRefs) : initialise());
  }

  /**
   * Clone or map `list`, one repo at a time. A repo that fails is marked and
   * the rest still run — the workspace exists either way, so the step ends
   * with Retry and Continue rather than with "Working…" forever.
   */
  async function runClones(list: RepoRef[]) {
    const ws = name.trim();
    err = "";
    initialising = true;
    done = false;
    progress = {
      ...progress,
      ...Object.fromEntries(list.map((r) => [r.name, { phase: "cloning", line: "waiting…" }])),
    };
    for (const r of list) {
      try {
        await hub.cloneOrMap({ workspace: ws, repo: r });
      } catch (e) {
        progress = { ...progress, [r.name]: { phase: "error", line: String(e) } };
      }
    }
    settle();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      dispatch("cancel");
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      if (done) {
        dispatch("created", { name: name.trim() });
        return;
      }
      void next();
    }
  }

  function focus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<svelte:window on:keydown={onKey} />

<div class="wiz" data-ui-state="new-workspace:{step}">
  <div class="card">
    <div class="stepn">{STEPS[idx].label}<span>{idx + 1} / {STEPS.length}</span></div>
    {#if step === "kind"}
      <p class="lede">Solo is this machine. Team shares the workspace through git.</p>
      <div class="choices">
        <button class="choice" class:on={kind === "solo"} on:click={() => (kind = "solo")}>
          <span class="c-title">Solo</span>
          <span class="c-note">Just this machine. Everything is files under ~/.parzi.</span>
        </button>
        <button class="choice" class:on={kind === "team"} on:click={() => (kind = "team")}>
          <span class="c-title">Team</span>
          <span class="c-note">Same files, shared through the workspace's git repo.</span>
        </button>
      </div>
      {#if kind === "team"}
        <p class="note">
          Round one syncs a team through git. The live layer — who holds which
          file, right now — arrives with the hub daemon in a later release.
        </p>
      {/if}
    {:else if step === "name"}
      <h2>Name it</h2>
      <p class="lede">Short and lowercase reads best in the sidebar.</p>
      <input
        class="in big"
        placeholder="acme"
        bind:value={name}
        use:focus
        on:keydown={(e) => e.stopPropagation()}
        on:keydown={(e) => { if (e.key === "Enter") next(); if (e.key === "Escape") dispatch("cancel"); }}
      />
    {:else if step === "github"}
      <h2>Connect GitHub</h2>
      <p class="lede">So Parzi can list your repos and clone the ones you pick.</p>
      <GithubConnect bind:status={gh} on:connected={(e) => (gh = e.detail.status)} />
    {:else if step === "orgs"}
      <h2>Where are the repos?</h2>
      <p class="lede">Pick your account, an organisation, or several.</p>
      <RepoPicker
        items={orgItems}
        bind:selected={pickedOrgs}
        loading={loadingOrgs}
        placeholder="Search organisations"
        empty="No organisations on this account."
      />
    {:else if step === "repos"}
      <h2>Pick the repos</h2>
      <p class="lede">These become the workspace. You can add more later.</p>
      <RepoPicker
        items={repoItems}
        bind:selected={pickedRepos}
        loading={loadingRepos}
        placeholder="Search repos"
        empty="No repos in those organisations."
      />
    {:else}
      <h2>{done ? "Ready" : stuck ? "Some repos did not clone" : "Setting up"}</h2>
      <p class="lede">
        {done
          ? `${name.trim()} is in the sidebar. Create a project in it to start work.`
          : stuck
            ? `${name.trim()} exists and knows its repos. Try the failed clones again, or continue and point them at a checkout you already have.`
            : `Cloning into ~/Parzi/${name.trim()}/ — this runs in the background.`}
      </p>
      <div class="clones">
        {#each Object.entries(progress) as [repo, p] (repo)}
          <div class="clone" class:bad={p.phase === "error"}>
            <span class="c-dot" class:spin={p.phase === "cloning"} class:ok={p.phase === "done"} />
            <span class="c-name">{repo}</span>
            <span class="c-line">{p.phase === "done" ? "ready" : p.line}</span>
          </div>
        {/each}
        {#if !Object.keys(progress).length}
          <div class="c-line">No repos selected — the workspace is created empty.</div>
        {/if}
      </div>
    {/if}

    {#if err}<div class="err">{err}</div>{/if}

    <div class="acts">
      <button class="btn ghost" on:click={() => (idx === 0 ? dispatch("cancel") : back())}>
        {idx === 0 ? "Cancel" : "Back"}
      </button>
      <span class="spacer" />
      {#if step === "init"}
        {#if stuck}
          <button class="btn" on:click={retry}>Retry</button>
          <button
            class="btn primary"
            disabled={!created}
            on:click={() => dispatch("created", { name: name.trim() })}
          >
            Continue without cloning
          </button>
        {:else}
          <button class="btn primary" disabled={!done} on:click={() => dispatch("created", { name: name.trim() })}>
            {done ? "Create a project" : "Working…"}
          </button>
        {/if}
      {:else}
        <button class="btn primary" disabled={!canNext} on:click={next}>
          {step === "repos" ? "Initialise" : "Continue"}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .wiz {
    display: flex; flex-direction: column; align-items: center; gap: 12px;
    padding: 16px 24px 18px; height: 100%; overflow-y: auto; box-sizing: border-box;
  }
  .stepn {
    display: flex; align-items: baseline; justify-content: space-between;
    font-size: 11px; color: var(--text-4); letter-spacing: 0.04em;
  }
  .stepn span { font-variant-numeric: tabular-nums; }
  .card {
    width: min(520px, 100%); display: flex; flex-direction: column; gap: 12px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-4); padding: 18px 20px;
  }
  h2 { margin: 0; font-size: 18px; font-weight: 600; color: var(--text); letter-spacing: -0.2px; }
  .lede { margin: -6px 0 2px; font-size: 12.5px; color: var(--text-3); line-height: 1.5; }
  .note {
    margin: 0; font-size: 11.5px; color: var(--text-3); line-height: 1.5;
    background: var(--surface-2); border-radius: var(--radius-2); padding: 9px 11px;
  }
  .choices { display: flex; gap: 10px; }
  .choice {
    flex: 1; display: flex; flex-direction: column; gap: 5px; text-align: left;
    background: var(--surface-2); border: 1px solid var(--line-2); border-radius: var(--radius-3);
    color: var(--text-2); font: inherit; padding: 13px 14px; cursor: pointer;
  }
  .choice:hover { background: var(--surface-3); }
  .choice.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--text); }
  .c-title { font-size: 13.5px; font-weight: 600; color: var(--text); }
  .c-note { font-size: 11.5px; color: var(--text-3); line-height: 1.45; }
  .in {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; padding: 9px 11px;
  }
  .in.big { font-size: 16px; padding: 11px 13px; }
  .in:focus { outline: none; border-color: var(--accent-line); }
  .clones { display: flex; flex-direction: column; gap: 6px; }
  .clone {
    display: flex; align-items: center; gap: 9px; font-size: 12.5px; color: var(--text-2);
    background: var(--surface-2); border-radius: var(--radius-2); padding: 8px 11px;
  }
  .clone.bad { background: var(--bad-soft); color: var(--bad); }
  .c-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-4); flex: none; }
  .c-dot.ok { background: var(--ok); }
  .c-dot.spin { background: var(--accent); animation: pulse 1.4s ease-in-out infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }
  .c-name { font-weight: 550; color: var(--text); }
  .c-line {
    flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 11.5px; color: var(--text-3); font-family: var(--parzi-mono);
  }
  .err {
    font-size: 12px; color: var(--bad); background: var(--bad-soft);
    border: 1px solid var(--bad-line); border-radius: var(--radius-2); padding: 7px 10px;
  }
  .acts { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
  .spacer { flex: 1; }
  .btn {
    background: var(--surface-2); border: 1px solid var(--line-3); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; font-weight: 500;
    padding: 8px 16px; cursor: pointer;
  }
  .btn:hover:not(:disabled) { background: var(--surface-3); }
  .btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn.ghost { background: transparent; }
  .btn.primary { background: var(--accent); border-color: transparent; color: var(--accent-ink); font-weight: 600; }
  .btn.primary:hover:not(:disabled) { filter: brightness(1.08); }
</style>
