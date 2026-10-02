<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let url = "";
  export let title = "Web Browser";

  let inputUrl = url;
  let loading = false;
  let iframeEl: HTMLIFrameElement | null = null;

  $: if (url) {
    inputUrl = url;
  }

  const dispatch = createEventDispatcher<{
    navigate: { url: string };
    openExternal: { url: string };
  }>();

  function onSubmit() {
    let target = inputUrl.trim();
    if (!target) return;
    if (!/^https?:\/\//i.test(target)) {
      if (target.includes(".") && !target.includes(" ")) {
        target = "https://" + target;
      } else {
        target = `https://duckduckgo.com/?q=${encodeURIComponent(target)}`;
      }
    }
    inputUrl = target;
    url = target;
    loading = true;
    dispatch("navigate", { url: target });
  }

  function reload() {
    if (iframeEl) {
      loading = true;
      iframeEl.src = url;
    }
  }

  function onIframeLoad() {
    loading = false;
  }

  function handleExternal() {
    if (url) {
      dispatch("openExternal", { url });
    }
  }
</script>

<div class="browser-viewport">
  <div class="browser-toolbar">
    <button class="nav-btn" title="Back" on:click={() => history.back()}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M19 12H5M12 19l-7-7 7-7" /></svg>
    </button>
    <button class="nav-btn" title="Forward" on:click={() => history.forward()}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 12h14M12 5l7 7-7 7" /></svg>
    </button>
    <button class="nav-btn" class:spin={loading} title="Reload" on:click={reload}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M23 4v6h-6M1 20v-6h6" /><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" /></svg>
    </button>

    <form class="url-form" on:submit|preventDefault={onSubmit}>
      <span class="url-scheme">🔒</span>
      <input
        type="text"
        bind:value={inputUrl}
        placeholder="Enter URL or search the web…"
        class="url-input"
      />
      {#if loading}
        <span class="load-spinner" />
      {/if}
    </form>

    <button class="nav-btn" title="Open in system browser" on:click={handleExternal}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14L21 3" /></svg>
    </button>
  </div>

  <div class="browser-body">
    {#if url}
      <iframe
        bind:this={iframeEl}
        src={url}
        title={title || "Web content"}
        class="web-frame"
        on:load={onIframeLoad}
        sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
      />
    {:else}
      <div class="blank-stage">
        <div class="blank-card">
          <div class="blank-icon">🌐</div>
          <h3>Web Browser</h3>
          <p>Enter a URL in the address bar above or search via the omnibar to explore the web alongside your agent harness.</p>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .browser-viewport {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    background: var(--stage, #0c0d12);
  }
  .browser-toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: rgba(16, 18, 23, 0.7);
    border-bottom: 1px solid var(--line-3, rgba(255, 255, 255, 0.07));
    backdrop-filter: blur(12px);
  }
  .nav-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-3);
    cursor: pointer;
    padding: 0;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .nav-btn:hover {
    background: var(--surface-2, rgba(255, 255, 255, 0.08));
    color: var(--text);
  }
  .nav-btn.spin svg {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  .url-form {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--surface-2, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--line-2, rgba(255, 255, 255, 0.1));
    border-radius: 6px;
    padding: 0 10px;
    height: 28px;
  }
  .url-scheme {
    font-size: 11px;
    opacity: 0.6;
  }
  .url-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text, #ffffff);
    font-size: 12px;
    font-family: inherit;
  }
  .load-spinner {
    width: 10px;
    height: 10px;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--accent, #7c8cff);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .browser-body {
    flex: 1;
    position: relative;
    overflow: hidden;
  }
  .web-frame {
    width: 100%;
    height: 100%;
    border: none;
    background: #ffffff;
  }
  .blank-stage {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 32px;
  }
  .blank-card {
    max-width: 440px;
    text-align: center;
    color: var(--text-3);
  }
  .blank-icon {
    font-size: 40px;
    margin-bottom: 12px;
  }
  .blank-card h3 {
    color: var(--text);
    font-size: 18px;
    margin: 0 0 8px;
  }
  .blank-card p {
    font-size: 13px;
    line-height: 1.5;
    margin: 0;
  }
</style>
