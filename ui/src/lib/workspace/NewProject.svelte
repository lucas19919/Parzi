<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { hub, type Roster, type Workspace } from "../api";
  import { ensureModels, modelRows } from "../modelStore";
  import ModelRolePicker, { defaultRoster } from "./ModelRolePicker.svelte";
  import RepoPicker, { type PickItem } from "./RepoPicker.svelte";

  export let workspace = "";
  export let initialStep = "";

  const dispatch = createEventDispatcher<{
    cancel: void;
    created: { workspace: string; slug: string };
  }>();

  type Step = "workspace" | "title" | "repos" | "roles";
  const STEPS: { id: Step; label: string }[] = [
    { id: "workspace", label: "Workspace" },
    { id: "title", label: "Title" },
    { id: "repos", label: "Repos" },
    { id: "roles", label: "Roles" },
  ];

  let step: Step = workspace ? "title" : "workspace";
  let names: string[] = [];
  let ws: Workspace | null = null;
  let title = "";
  let pickedRepos: string[] = [];
  let roster: Roster = { header: "", orchestrator: "", coder: "" };
  let loading = true;
  let creating = false;
  let err = "";

  onMount(async () => {
    const want = initialStep.trim();
    if (want && STEPS.some((s) => s.id === want)) step = want as Step;
    try {
      names = await hub.workspaces();
      if (!workspace && names.length === 1) workspace = names[0];
      if (workspace) await loadWorkspace();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
    try {
      roster = defaultRoster(await ensureModels());
    } catch {}
  });

  async function loadWorkspace() {
    ws = await hub.workspace(workspace);
    pickedRepos = ws.repos.map((r) => r.name);
  }

  $: idx = STEPS.findIndex((s) => s.id === step);
  $: wsItems = names.map((n) => ({ key: n, name: n })) as PickItem[];
  $: repoItems = (ws?.repos ?? []).map((r) => ({
    key: r.name,
    name: r.name,
    sub: r.local_path ?? r.remote,
    badge: r.local_path ? "" : "not cloned",
  })) as PickItem[];
  $: canNext =
    (step === "workspace" && !!workspace) ||
    (step === "title" && !!title.trim()) ||
    (step === "repos" && (pickedRepos.length > 0 || !repoItems.length)) ||
    step === "roles";

  async function next() {
    if (!canNext || creating) return;
    err = "";
    if (step === "workspace") {
      try {
        await loadWorkspace();
      } catch (e) {
        err = String(e);
        return;
      }
      step = "title";
      return;
    }
    if (step === "title") { step = "repos"; return; }
    if (step === "repos") { step = "roles"; return; }
    await create();
  }

  function back() {
    if (idx > 0) step = STEPS[idx - 1].id;
    else dispatch("cancel");
  }

  async function create() {
    creating = true;
    err = "";
    try {
      const p = await hub.createProject({
        workspace,
        title: title.trim(),
        repos: pickedRepos,
        roster,
      });
      dispatch("created", { workspace, slug: p.slug });
    } catch (e) {
      err = String(e);
      creating = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t?.closest("input, textarea, select, [contenteditable]")) return;
    if (e.key === "Escape") {
      e.preventDefault();
      dispatch("cancel");
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) void next();
  }

  function focus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  /** One-workspace picker: choosing is also advancing. */
  function pickWorkspace(name: string) {
    workspace = name;
    void next();
  }
</script>

<svelte:window on:keydown={onKey} />

<div class="wiz" data-ui-state="new-project:{step}">
  <div class="card">
    <div class="stepn">{STEPS[idx].label}<span>{idx + 1} / {STEPS.length}</span></div>
    <div class="body">
      {#if step === "workspace"}
        <p class="lede">Which workspace this belongs to.</p>
        {#if loading}
          <div class="msg">Loading workspaces…</div>
        {:else if !names.length}
          <div class="msg">No workspace yet — create one first.</div>
        {:else}
          <div class="rows">
            {#each wsItems as w (w.key)}
              <button class="row" class:on={w.key === workspace} on:click={() => pickWorkspace(w.key)}>
                <span class="r-name">{w.name}</span>
              </button>
            {/each}
          </div>
        {/if}
      {:else if step === "title"}
        <input
          class="in big"
          placeholder="Checkout flow"
          bind:value={title}
          use:focus
          on:keydown={(e) => e.stopPropagation()}
          on:keydown={(e) => { if (e.key === "Enter") next(); if (e.key === "Escape") dispatch("cancel"); }}
        />
      {:else if step === "repos"}
        {#if repoItems.length}
          <RepoPicker
            items={repoItems}
            bind:selected={pickedRepos}
            placeholder="Search repos"
            empty="This workspace has no repos yet."
          />
        {:else}
          <div class="msg">{workspace} has no repos yet — add some to the workspace first.</div>
        {/if}
      {:else if step === "roles"}
        <ModelRolePicker label="Header" rows={$modelRows} bind:value={roster.header} />
        <ModelRolePicker label="Orchestrator" rows={$modelRows} bind:value={roster.orchestrator} />
        <ModelRolePicker label="Coder" rows={$modelRows} bind:value={roster.coder} />
      {/if}

      {#if err}<div class="err">{err}</div>{/if}
    </div>

    <div class="acts">
      <button class="btn ghost" on:click={back}>{idx === 0 ? "Cancel" : "Back"}</button>
      <span class="spacer" />
      <button class="btn primary" disabled={!canNext || creating} on:click={next}>
        {#if step === "roles"}{creating ? "Creating…" : "Create"}{:else}Continue{/if}
      </button>
    </div>
  </div>
</div>

<style>
  .wiz {
    display: flex; flex-direction: column; align-items: center; gap: 12px;
    padding: 16px 24px 18px; height: 100%; overflow: hidden; box-sizing: border-box;
  }
  .stepn {
    display: flex; align-items: baseline; justify-content: space-between;
    font-size: 11px; color: var(--text-4); letter-spacing: 0.04em;
  }
  .stepn span { font-variant-numeric: tabular-nums; }
  .card {
    width: min(520px, 100%); display: flex; flex-direction: column; gap: 12px;
    flex: 0 1 auto; min-height: 0; max-height: 100%; overflow: hidden;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-4); padding: 18px 20px;
  }
  .body {
    flex: 1 1 auto; min-height: 0; overflow-y: auto;
    display: flex; flex-direction: column; gap: 10px;
    margin-right: -8px; padding-right: 8px;
  }
  .lede { margin: 0; font-size: 12px; color: var(--text-3); line-height: 1.4; }
  .msg { font-size: 12.5px; color: var(--text-3); padding: 14px 10px; text-align: center; }
  .rows { display: flex; flex-direction: column; gap: 4px; }
  .row {
    display: flex; align-items: center; gap: 9px; text-align: left;
    background: var(--surface-2); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text-2); font: inherit; font-size: 13px; padding: 10px 12px; cursor: pointer;
  }
  .row:hover { background: var(--surface-3); color: var(--text); }
  .row.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--text); }
  .r-name { font-weight: 550; }
  .in {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; padding: 9px 11px;
  }
  .in.big { font-size: 16px; padding: 11px 13px; }
  .in:focus { outline: none; border-color: var(--accent-line); }
  .err {
    font-size: 12px; color: var(--bad); background: var(--bad-soft);
    border: 1px solid var(--bad-line); border-radius: var(--radius-2); padding: 7px 10px;
  }
  .acts {
    display: flex; align-items: center; gap: 8px; flex: none;
    border-top: 1px solid var(--line-2); padding-top: 10px;
  }
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
