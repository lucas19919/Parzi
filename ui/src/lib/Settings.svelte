<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import SystemSection from "./settings/SystemSection.svelte";
  import { toast } from "./toast";

  export let section = "general";

  const dispatch = createEventDispatcher<{ close: void }>();

  const SECTIONS = [
    { id: "general", label: "General" },
    { id: "providers", label: "Providers" },
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
    <button class="close" title="Close settings (Esc)" on:click={() => dispatch("close")}>
      <Icon name="close" size={13} stroke={2} />
    </button>
  </nav>
  <div class="scroll">
    <div class="content">
      {#if section === "providers"}
        <ProvidersSection {notify} />
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
  .close {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--line);
    color: var(--text);
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
