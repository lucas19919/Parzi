<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { api } from "./api";
  import { tooltip } from "./tooltip";
  import { hostLabel, type HostedSite } from "./hosted";

  export let site: HostedSite;

  const dispatch = createEventDispatcher<{ close: void; openUrl: { url: string } }>();

  // Is the server still listening? The backend checks the port.
  let up: boolean | null = null;
  async function probe() {
    const url = site.url;
    const got = await api.urlListening(url).catch(() => null);
    if (url === site.url) up = got;
  }
  $: site.url, (up = null), void probe();

  onMount(() => {
    const timer = setInterval(probe, 10_000);
    return () => clearInterval(timer);
  });
</script>

<InfoCard id="hosted" title="Hosted">
  <svelte:fragment slot="aside">{up === false ? "stopped" : up ? "live" : ""}</svelte:fragment>
  <svelte:fragment slot="actions">
    <button
      class="icon-btn"
      use:tooltip={"Open in the system browser"}
      aria-label="Open in the system browser"
      on:click={() => api.openConfirmedUrl(site.url).catch(() => {})}
    >
      <Icon name="external" size={12} />
    </button>
    <button class="icon-btn" use:tooltip={"Unpin"} aria-label="Unpin" on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
  </svelte:fragment>
  <button class="info-line row" use:tooltip={`Open ${site.url} in Parzi`} on:click={() => dispatch("openUrl", { url: site.url })}>
    <i class="info-dot" class:up={up === true} class:down={up === false} />
    <span class="info-v">{site.title}</span>
    <span class="info-time mono">{hostLabel(site.url)}</span>
  </button>
</InfoCard>

<style>
  .info-dot {
    background: var(--muted);
  }
  .info-dot.up {
    background: var(--ok);
  }
  .info-dot.down {
    background: var(--bad);
  }
</style>
