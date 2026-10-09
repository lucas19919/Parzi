<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import type { AgentRequest } from "./api";

  export let request: AgentRequest;
  export let title = "Agent";
  export let pending = 0;

  const dispatch = createEventDispatcher<{
    answer: { key: string; text: string };
    decline: { key: string };
  }>();

  const KINDS: Record<string, { icon: "file" | "image" | "chat"; label: string }> = {
    file: { icon: "file", label: "wants a file" },
    image: { icon: "image", label: "wants a picture" },
    text: { icon: "chat", label: "asks you" },
  };
  $: kind = KINDS[request.kind] ?? KINDS.text;

  let draft = "";
  let field: HTMLTextAreaElement | null = null;

  async function send() {
    const text = draft.trim();
    if (!text) return;
    dispatch("answer", { key: request.key, text });
    draft = "";
  }

  function decline() {
    dispatch("decline", { key: request.key });
    draft = "";
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      decline();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void send();
    }
  }

  $: if (field && request) void tick().then(() => field?.focus());
</script>

<svelte:window on:keydown|capture={onKey} />

<div class="scrim" transition:fade={{ duration: 140 }} role="presentation">
  <div
    class="card"
    role="dialog"
    aria-modal="true"
    aria-label="Agent request"
    transition:fly={{ y: 10, duration: 180, easing: cubicOut }}
  >
    <div class="head">
      <span class="kind"><Icon name={kind.icon} size={14} /></span>
      <span class="who">{title} {kind.label}</span>
      {#if pending > 1}<span class="count">{pending} waiting</span>{/if}
    </div>
    <p class="body">{request.request}</p>
    <textarea
      bind:this={field}
      bind:value={draft}
      rows="3"
      placeholder={request.kind === "file" ? "Paste the file path…" : request.kind === "image" ? "Describe it, or drop the picture in the composer…" : "Type your reply… (Ctrl+Enter sends)"}
      spellcheck="true"
    />
    <div class="foot">
      <button class="btn" on:click={decline}>Decline</button>
      <button class="btn primary" disabled={!draft.trim()} on:click={send}>Send</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    background: var(--scrim);
  }
  .card {
    width: min(480px, 100%);
    padding: 16px 16px 12px;
    background: var(--glass-strong-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
    border: var(--glass-hairline);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .kind {
    display: inline-flex;
    color: var(--accent);
  }
  .who {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 650;
    color: var(--text);
  }
  .count {
    flex: none;
    font-size: 11px;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .body {
    margin: 0 0 10px;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--text);
    white-space: pre-wrap;
  }
  textarea {
    width: 100%;
    min-height: 64px;
    max-height: 160px;
    padding: 8px 10px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
    outline: none;
  }
  textarea:focus {
    border-color: var(--accent);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
</style>
