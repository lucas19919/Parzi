<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { api, type ProjectRoster } from "./api";

  export let project = "default";
  export let roster: ProjectRoster | null = null;

  const dispatch = createEventDispatcher<{ saved: { roster: ProjectRoster } }>();

  let draft: ProjectRoster = {
    header: { model: "", effort: "", system_prompt: "", temperature: null },
    orchestrator: { model: "", effort: "", system_prompt: "", temperature: null },
    implementation: { model: "", effort: "", system_prompt: "", temperature: null },
  };
  let saving = false;
  let err = "";

  $: if (roster) {
    draft = JSON.parse(JSON.stringify(roster));
    for (const k of ["header", "orchestrator", "implementation"] as const) {
      draft[k] = { model: "", effort: "", system_prompt: "", temperature: null, ...(draft[k] ?? {}) };
    }
  }

  type RoleKey = "header" | "orchestrator" | "implementation";
  const roles: { key: RoleKey; label: string }[] = [
    { key: "header", label: "Header · Architect" },
    { key: "orchestrator", label: "Orchestrator · PM" },
    { key: "implementation", label: "Implementation · Builder" },
  ];

  async function save() {
    saving = true;
    err = "";
    try {
      const clean: ProjectRoster = JSON.parse(JSON.stringify(draft));
      for (const k of ["header", "orchestrator", "implementation"] as const) {
        for (const f of ["model", "effort", "system_prompt"] as const) {
          const v = (clean[k] as Record<string, unknown>)[f];
          if (typeof v === "string" && !v.trim()) (clean[k] as Record<string, unknown>)[f] = null;
        }
      }
      await api.saveProjectRoster(project, clean);
      dispatch("saved", { roster: clean });
    } catch (e) {
      err = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="roster" aria-label="Agent roster">
  {#each roles as { key, label }}
    <div class="role">
      <div class="role-label">{label}</div>
      <input
        class="role-input"
        placeholder="model (blank = auto)"
        bind:value={draft[key].model}
        aria-label="{label} model"
      />
      <input
        class="role-input small"
        placeholder="effort"
        bind:value={draft[key].effort}
        aria-label="{label} effort"
      />
    </div>
  {/each}
  <button class="save-btn" on:click={save} disabled={saving}>{saving ? "Saving…" : "Save roster"}</button>
  {#if err}<span class="err">{err}</span>{/if}
</div>

<style>
  .roster {
    display: flex; align-items: flex-end; gap: 10px; flex-wrap: wrap;
    background: var(--surface-1); border: 1px solid var(--line);
    border-radius: 12px; padding: 12px 14px;
  }
  .role { display: flex; flex-direction: column; gap: 6px; min-width: 180px; flex: 1; }
  .role-label { font-size: 11px; font-weight: 700; letter-spacing: 0.4px; text-transform: uppercase; color: var(--text-3); }
  .role-input {
    background: var(--surface-2); border: 1px solid var(--line); color: var(--text);
    border-radius: 7px; padding: 7px 10px; font: inherit; font-size: 12.5px;
  }
  .role-input.small { max-width: 160px; }
  .save-btn {
    background: var(--accent-soft); border: 1px solid var(--accent-line); color: var(--accent-text);
    font: inherit; font-size: 12px; padding: 7px 12px; border-radius: 7px; cursor: pointer; white-space: nowrap;
  }
  .save-btn:disabled { opacity: 0.6; cursor: default; }
  .err { color: var(--bad); font-size: 11.5px; }
</style>
