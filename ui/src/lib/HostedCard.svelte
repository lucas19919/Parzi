<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { api } from "./api";
  import { tooltip } from "./tooltip";
  import type { HostedSite } from "./hosted";

  export let site: HostedSite;

  const dispatch = createEventDispatcher<{ close: void }>();

  // Reloading swaps the frame for a fresh one.
  let bust = 0;
</script>

<InfoCard id="hosted" title={site.title}>
  <svelte:fragment slot="aside">live</svelte:fragment>
  <svelte:fragment slot="actions">
    <button class="icon-btn" use:tooltip={"Reload"} aria-label="Reload" on:click={() => (bust += 1)}><Icon name="reload" size={12} /></button>
    <button
      class="icon-btn"
      use:tooltip={"Open outside Parzi, in the system browser"}
      aria-label="Open outside Parzi"
      on:click={() => api.openConfirmedUrl(site.url).catch(() => {})}
    >
      <Icon name="external" size={12} />
    </button>
    <button class="icon-btn" use:tooltip={"Unpin"} aria-label="Unpin" on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
  </svelte:fragment>
  {#key bust}
    <iframe class="frame" title={site.title} src={site.url} sandbox="allow-scripts allow-same-origin" loading="lazy" />
  {/key}
</InfoCard>

<style>
  .frame {
    display: block;
    width: 100%;
    height: 320px;
    border: 1px solid var(--line);
    border-radius: var(--radius-in);
    background: var(--bg);
  }
</style>
