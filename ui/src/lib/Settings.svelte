<script lang="ts">
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import SystemSection from "./settings/SystemSection.svelte";
  import ConnectionsSection from "./settings/ConnectionsSection.svelte";
  import { toast } from "./toast";

  export let section = "general";

  const SECTIONS = [
    { id: "general", label: "General" },
    { id: "providers", label: "Providers" },
    { id: "connections", label: "Connections" },
    { id: "appearance", label: "Appearance" },
    { id: "system", label: "System" },
  ];

  const notify = (msg: string) => toast(msg);
</script>

<div class="settings">
  <nav>
    <div class="sections" role="tablist">
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
      {:else if section === "system"}
        <SystemSection {notify} />
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
    padding: 6px 16px;
    border-bottom: 1px solid var(--line);
  }
  .sections {
    display: flex;
    gap: 4px;
    overflow-x: auto;
  }
  .sections button {
    padding: 5px 11px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
  }
  .sections button:hover {
    background: var(--line);
    color: var(--text);
  }
  .sections button.active {
    background: var(--line);
    color: var(--text);
    font-weight: 500;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 24px;
    width: 100%;
    max-width: 980px;
    margin: 0 auto;
    padding: 26px 28px 40px;
    user-select: none;
  }
</style>
