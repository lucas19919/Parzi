<script lang="ts">
  import { onMount, tick } from "svelte";
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import ConnectionsSection from "./settings/ConnectionsSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import { toast } from "./toast";

  // One long page, docs-style: the side lists every section, the page
  // scrolls to it. "system" is legacy (merged into general) and lands there.
  export let section = "general";

  const NAV = [
    { id: "general", label: "General" },
    { id: "providers", label: "Providers" },
    { id: "connections", label: "Connections" },
    { id: "appearance", label: "Appearance" },
  ];

  const notify = (msg: string, err = false) => toast(msg, err);

  let active = "general";
  let scrollEl: HTMLElement | null = null;

  const anchorOf = (s: string) => (s === "system" ? "general" : s);

  function go(id: string) {
    active = id;
    scrollEl?.querySelector(`[data-anchor="${id}"]`)?.scrollIntoView({ block: "start" });
  }

  // A new section asked for (update banner, model picker, switcher) scrolls
  // there; the spy below takes over from the user's own scrolling.
  let lastTarget = "";
  $: target = anchorOf(section);
  $: if (target && target !== lastTarget) {
    lastTarget = target;
    active = target;
    void tick().then(() => {
      scrollEl?.querySelector(`[data-anchor="${target}"]`)?.scrollIntoView({ block: "start" });
    });
  }

  onMount(() => {
    const boxes = () =>
      [...(scrollEl?.querySelectorAll("[data-anchor]") ?? [])] as HTMLElement[];
    const spy = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) active = (e.target as HTMLElement).dataset.anchor ?? active;
        }
      },
      { root: scrollEl, rootMargin: "-12% 0px -72% 0px" },
    );
    void tick().then(() => boxes().forEach((b) => spy.observe(b)));
    return () => spy.disconnect();
  });
</script>

<div class="settings">
  <nav class="side" aria-label="Settings sections">
    {#each NAV as n (n.id)}
      <button class:active={active === n.id} aria-current={active === n.id ? "true" : undefined} on:click={() => go(n.id)}>
        {n.label}
      </button>
    {/each}
  </nav>
  <div class="scroll" bind:this={scrollEl}>
    <div class="content">
      <section class="anchor" data-anchor="general" aria-label="General">
        <GeneralSection {notify} />
      </section>
      <section class="anchor" data-anchor="providers" aria-label="Providers">
        <ProvidersSection {notify} />
      </section>
      <section class="anchor" data-anchor="connections" aria-label="Connections">
        <ConnectionsSection {notify} on:setupRemote />
      </section>
      <section class="anchor" data-anchor="appearance" aria-label="Appearance">
        <AppearanceSection {notify} />
      </section>
    </div>
  </div>
</div>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    display: flex;
    background: transparent;
  }
  /* The docs margin: one quiet column of links, current one lit. */
  .side {
    flex: none;
    width: 168px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-items: stretch;
    padding: 26px 0 40px 28px;
    overflow-y: auto;
  }
  .side button {
    padding: 5px 10px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .side button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .side button.active {
    color: var(--text);
    background: var(--selected);
    font-weight: 600;
  }
  .scroll {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 44px;
    width: 100%;
    max-width: 760px;
    margin: 0 auto;
    padding: 26px 28px 48px;
  }
  .anchor {
    display: flex;
    flex-direction: column;
    gap: 26px;
    scroll-margin-top: 8px;
  }
</style>
