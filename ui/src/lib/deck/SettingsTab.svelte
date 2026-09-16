<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { PROJECT_STATUSES, type Project } from "../api";
  import { onMount } from "svelte";
  import { ensureModels, modelRows } from "../modelStore";
  import ModelPicker from "../ModelPicker.svelte";

  export let project: Project;

  onMount(() => { void ensureModels(false); });

  const dispatch = createEventDispatcher<{ save: { project: Project } }>();

  let draft: Project = structuredClone(project);
  let saving = false;
  $: if (project.slug !== draft.slug || project.workspace !== draft.workspace) {
    draft = structuredClone(project);
  }

  $: dirty = JSON.stringify(draft) !== JSON.stringify(project);

  function addWhat() {
    draft.what = [...draft.what, { text: "", done: false }];
  }
  function addConstraint() {
    draft.constraints = [...draft.constraints, ""];
  }
  function addCritical() {
    draft.critical = [...draft.critical, ""];
  }

  async function save() {
    if (saving || !dirty) return;
    saving = true;
    dispatch("save", {
      project: {
        ...draft,
        what: draft.what.filter((w) => w.text.trim()),
        constraints: draft.constraints.map((c) => c.trim()).filter(Boolean),
        critical: draft.critical.map((c) => c.trim()).filter(Boolean),
      },
    });
    saving = false;
  }
</script>

<div class="set">
  <label class="fld">
    <span>Title</span>
    <input class="in" bind:value={draft.title} />
  </label>
  <label class="fld">
    <span>Status</span>
    <select class="in" bind:value={draft.status}>
      {#each PROJECT_STATUSES as s}
        <option value={s}>{s}</option>
      {/each}
    </select>
  </label>

  <div class="fld">
    <span>Header</span>
    <ModelPicker label="Header" models={$modelRows} bind:value={draft.roster.header} />
  </div>
  <div class="fld">
    <span>Orchestrator</span>
    <ModelPicker label="Orchestrator" models={$modelRows} bind:value={draft.roster.orchestrator} />
  </div>
  <div class="fld">
    <span>Coder</span>
    <ModelPicker label="Coder" models={$modelRows} bind:value={draft.roster.coder} />
  </div>

  <label class="fld">
    <span>Why</span>
    <textarea class="in area" rows="4" bind:value={draft.why} />
  </label>

  <div class="fld">
    <span>What</span>
    {#each draft.what as w, i}
      <div class="row">
        <input type="checkbox" bind:checked={w.done} />
        <input class="in" bind:value={w.text} />
        <button class="x" on:click={() => (draft.what = draft.what.filter((_, j) => j !== i))}>×</button>
      </div>
    {/each}
    <button class="add" on:click={addWhat}>Add criterion</button>
  </div>

  <div class="fld">
    <span>Constraints</span>
    {#each draft.constraints as _, i}
      <div class="row">
        <input class="in" bind:value={draft.constraints[i]} />
        <button class="x" on:click={() => (draft.constraints = draft.constraints.filter((_, j) => j !== i))}>×</button>
      </div>
    {/each}
    <button class="add" on:click={addConstraint}>Add constraint</button>
  </div>

  <div class="fld">
    <span>Critical paths</span>
    {#each draft.critical as _, i}
      <div class="row">
        <input class="in" bind:value={draft.critical[i]} placeholder="shop-api/src/payments/**" />
        <button class="x" on:click={() => (draft.critical = draft.critical.filter((_, j) => j !== i))}>×</button>
      </div>
    {/each}
    <button class="add" on:click={addCritical}>Add glob</button>
  </div>

  <button class="save" disabled={!dirty || saving} on:click={save}>{saving ? "Saving…" : "Save"}</button>
</div>

<style>
  .set { display: flex; flex-direction: column; gap: 14px; max-width: 560px; padding: 8px 2px 48px; }
  .fld { display: flex; flex-direction: column; gap: 6px; font-size: 12px; color: var(--text-3); font-weight: 600; }
  .in {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-weight: 400; padding: 8px 10px;
  }
  .in.area { resize: vertical; min-height: 72px; line-height: 1.5; }
  .row { display: flex; align-items: center; gap: 6px; }
  .row .in { flex: 1; min-width: 0; }
  .x {
    flex: none; width: 26px; height: 26px; background: none; border: none; color: var(--text-4);
    cursor: pointer; border-radius: 6px;
  }
  .x:hover { color: var(--text); background: var(--surface-2); }
  .add {
    align-self: flex-start; background: none; border: none; color: var(--text-3);
    font: inherit; font-size: 12px; cursor: pointer; padding: 0;
  }
  .add:hover { color: var(--text); }
  .save {
    align-self: flex-start; background: var(--accent); border: none; color: var(--accent-ink);
    font: inherit; font-size: 12.5px; font-weight: 600; padding: 8px 16px;
    border-radius: var(--radius-2); cursor: pointer;
  }
  .save:disabled { opacity: 0.4; cursor: default; }
</style>
