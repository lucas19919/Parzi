<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { covered } from "./overlay";
  import { addPin, pins, removePin } from "./browserData";
  import { toAddress, type Tab } from "./tabs";

  export let tab: Tab;
  export let immersive = false;

  const dispatch = createEventDispatcher<{ navigate: { url: string }; error: { text: string } }>();

  let slot: HTMLDivElement | null = null;
  let field: HTMLInputElement | null = null;
  let address = tab.url ?? "";
  let editing = false;
  let frame = 0;
  let alive = true;
  let reported = false;
  let chain: Promise<void> = Promise.resolve();

  $: url = tab.url ?? "";
  $: if (!editing) address = url;
  $: secure = url.startsWith("https://");
  $: pinned = $pins.some((p) => p.url === url);
  $: url, immersive, $covered, slot, schedule();

  export async function focusAddress() {
    await tick();
    field?.focus();
    field?.select();
  }

  function schedule() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      chain = chain.then(sync).catch(() => {});
    });
  }

  async function sync() {
    if (!alive || !slot) return;
    const r = slot.getBoundingClientRect();
    try {
      if ($covered || !url || r.width < 8 || r.height < 8) {
        await api.browserHide();
        return;
      }
      const rect = { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
      await api.browserShow(tab.id, rect, url);
    } catch (e) {
      if (!reported) {
        reported = true;
        dispatch("error", { text: String(e) });
      }
    }
  }

  function go() {
    const next = toAddress(address);
    if (!next) return;
    editing = false;
    field?.blur();
    dispatch("navigate", { url: next });
  }

  function nav(action: "back" | "forward" | "reload" | "stop") {
    api.browserNav(tab.id, action).catch((e) => dispatch("error", { text: String(e) }));
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      editing = false;
      address = url;
      field?.blur();
    }
  }

  function togglePin() {
    if (pinned) removePin(url);
    else addPin(url, tab.title);
  }

  onMount(() => {
    const ro = new ResizeObserver(schedule);
    if (slot) ro.observe(slot);
    window.addEventListener("resize", schedule);
    if (!url) void focusAddress();
    return () => {
      alive = false;
      cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", schedule);
    };
  });
</script>

<div class="page">
  {#if !immersive}
    <div class="toolbar">
      <button class="nav" title="Back (Alt+Left)" disabled={!tab.canGoBack} on:click={() => nav("back")}>
        <Icon name="arrowLeft" size={15} />
      </button>
      <button class="nav" title="Forward (Alt+Right)" disabled={!tab.canGoForward} on:click={() => nav("forward")}>
        <Icon name="arrowRight" size={15} />
      </button>
      {#if tab.loading}
        <button class="nav" title="Stop" on:click={() => nav("stop")}><Icon name="close" size={15} /></button>
      {:else}
        <button class="nav" title="Reload (Ctrl+R)" disabled={!url} on:click={() => nav("reload")}>
          <Icon name="reload" size={14} />
        </button>
      {/if}
      <form class="address" class:editing on:submit|preventDefault={go}>
        {#if url && !editing}
          <span class="scheme" class:secure title={secure ? "Secure connection" : "Not secure"}>
            <Icon name={secure ? "lock" : "unlock"} size={12} />
          </span>
        {/if}
        <input
          bind:this={field}
          bind:value={address}
          on:focus={() => {
            editing = true;
            field?.select();
          }}
          on:blur={() => (editing = false)}
          on:keydown={onKey}
          placeholder="Search or enter an address"
          spellcheck="false"
          autocomplete="off"
        />
        {#if tab.loading}<span class="progress" />{/if}
      </form>
      <button class="nav" class:on={pinned} title={pinned ? "Unpin from Home" : "Pin to Home"} disabled={!url} on:click={togglePin}>
        <Icon name="pin" size={14} />
      </button>
    </div>
  {/if}
  <div class="slot" class:empty={!url} bind:this={slot}>
    {#if !url}
      <div class="blank">Type an address or a search above</div>
    {/if}
  </div>
</div>

<style>
  .page {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px;
    border-bottom: 1px solid var(--line);
  }
  .nav {
    width: 30px;
    height: 30px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .nav:hover:not(:disabled) {
    background: var(--line);
    color: var(--text);
  }
  .nav:disabled {
    color: var(--faint);
    cursor: default;
  }
  .nav.on {
    color: var(--accent);
  }
  .address {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 6px;
    padding: 0 10px;
    background: var(--panel);
    border: 1px solid transparent;
    border-radius: 999px;
    overflow: hidden;
  }
  .address:hover {
    background: var(--line);
  }
  .address.editing {
    background: var(--bg);
    border-color: var(--accent);
  }
  .scheme {
    display: inline-flex;
    flex: none;
    color: var(--warn);
  }
  .scheme.secure {
    color: var(--faint);
  }
  .address input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: 13px;
  }
  .address input::placeholder {
    color: var(--faint);
  }
  .progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 30%;
    background: var(--accent);
    animation: sweep 1.1s ease-in-out infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .slot {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #fff;
  }
  .slot.empty {
    background: transparent;
  }
  .blank {
    font-size: 12.5px;
    color: var(--faint);
  }
</style>
