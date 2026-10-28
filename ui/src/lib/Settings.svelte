<script lang="ts">
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import ConnectionsSection from "./settings/ConnectionsSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import { toast } from "./toast";

  export let section = "general";

  const SECTIONS = [
    { id: "general", label: "General" },
    { id: "providers", label: "Providers" },
    { id: "connections", label: "Connections" },
    { id: "appearance", label: "Appearance" },
  ];

  const notify = (msg: string, err = false) => toast(msg, err);
</script>

<div class="settings">
  <nav>
    <div class="sections page-tabs" role="tablist" aria-label="Settings sections">
      {#each SECTIONS as s (s.id)}
        <button role="tab" aria-selected={section === s.id} class:active={section === s.id} on:click={() => (section = s.id)}>
          {s.label}
        </button>
      {/each}
    </div>
  </nav>
  <div class="scroll">
    <div class="content">
      {#if section === "providers"}
        <ProvidersSection {notify} />
      {:else if section === "connections"}
        <ConnectionsSection {notify} on:setupRemote />
      {:else if section === "appearance"}
        <AppearanceSection {notify} />
      {:else}
        <GeneralSection {notify} />
      {/if}
    </div>
  </div>
</div>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: transparent;
  }
  nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    /* First tab shares the session's top-left corner. */
    padding: calc(var(--radius) - var(--radius-sm)) calc(var(--space) * 4);
    padding-left: calc(var(--radius) - var(--radius-sm));
    border-bottom: 1px solid var(--line);
  }
  .sections {
    overflow-x: auto;
  }
  .sections > button {
    border-radius: var(--radius-sm);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 6);
    width: 100%;
    max-width: 840px;
    margin: 0 auto;
    padding: calc(var(--space) * 6) calc(var(--space) * 8) calc(var(--space) * 8);
  }
</style>
