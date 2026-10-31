<script lang="ts">
  import Page from "./Page.svelte";
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import ConnectionsSection from "./settings/ConnectionsSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import { toast } from "./toast";

  export let section = "general";
  export let onSetup: () => void = () => {};
  export let onReport: () => void = () => {};

  const SECTIONS = [
    { id: "general", label: "General" },
    { id: "providers", label: "Providers" },
    { id: "connections", label: "Connectors" },
    { id: "appearance", label: "Appearance" },
  ];

  const notify = (msg: string, err = false) => toast(msg, err);
</script>

<Page>
  <div slot="bar" class="sections page-tabs" role="tablist" aria-label="Settings sections">
    {#each SECTIONS as s (s.id)}
      <button role="tab" aria-selected={section === s.id} class:active={section === s.id} on:click={() => (section = s.id)}>
        {s.label}
      </button>
    {/each}
  </div>
  {#if section === "providers"}
    <ProvidersSection {notify} />
  {:else if section === "connections"}
    <ConnectionsSection which="apps" {notify} />
  {:else if section === "appearance"}
    <AppearanceSection {notify} />
  {:else}
    <GeneralSection {notify} {onSetup} {onReport} on:setupRemote />
  {/if}
</Page>
