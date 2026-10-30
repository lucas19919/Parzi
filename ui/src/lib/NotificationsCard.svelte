<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import InfoCard from "./InfoCard.svelte";
  import { notifications, markAllRead, markRead, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  let listEl: HTMLDivElement | null = null;
  let cut = false;

  async function measure() {
    await tick();
    const el = listEl;
    cut = !!el && el.scrollHeight - el.scrollTop - el.clientHeight > 8;
  }

  $: if ($notifications) void measure();

  $: unread = $notifications.filter((n) => !n.read).length;

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return "just now";
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h`;
    return `${Math.floor(h / 24)}d`;
  }

  function open(n: NotificationItem) {
    markRead(n.id);
    if (n.sessionId) dispatch("openSession", { id: n.sessionId });
  }
</script>

{#if $notifications.length}
  <div class="pin">
    <InfoCard id="notifications" title="Notifications">
      <svelte:fragment slot="aside">{#if unread}<span class="fresh"><b>{unread}</b> new</span>{/if}</svelte:fragment>
      <div class="clip" class:cut>
        <div class="list" bind:this={listEl} on:scroll={measure}>
          {#each $notifications as n (n.id)}
            <button class="item" class:unread={!n.read} on:click={() => open(n)}>
              <i class="dot" />
              <span class="name">{n.title}</span>
              <span class="time">{timeAgo(n.time)}</span>
              {#if n.body}<span class="body">{n.body}</span>{/if}
            </button>
          {/each}
        </div>
      </div>
      <div class="acts">
        {#if unread}<button type="button" on:click={markAllRead}>Mark read</button>{/if}
        <button type="button" on:click={clearNotifications}>Clear</button>
      </div>
    </InfoCard>
  </div>
{/if}

<style>
  .pin {
    position: sticky;
    top: 0;
    z-index: 2;
    flex: none;
  }
  .clip {
    position: relative;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    max-height: calc(var(--space) * 48);
    overflow-y: auto;
  }
  .clip.cut .list {
    -webkit-mask-image: linear-gradient(to bottom, black calc(100% - 36px), transparent);
    mask-image: linear-gradient(to bottom, black calc(100% - 36px), transparent);
  }
  .clip.cut::after {
    content: "";
    position: absolute;
    left: 0;
    right: 10px;
    bottom: 0;
    height: 36px;
    pointer-events: none;
    background: linear-gradient(to bottom, transparent, var(--panel));
  }
  .fresh b {
    color: var(--text);
    font-weight: 600;
  }
  .item {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: calc(var(--space) * 2);
    row-gap: calc(var(--space) / 2);
    width: 100%;
    min-height: 36px;
    margin: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .dot {
    align-self: center;
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
  }
  .unread .dot {
    background: var(--accent);
  }
  .unread {
    color: var(--text);
  }
  .unread .name {
    font-weight: 600;
    color: var(--text);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    color: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .body {
    flex: 1 1 100%;
    margin-left: calc(var(--space) * 4);
    color: var(--muted);
    font-size: var(--fs-xs);
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
  }
  .acts button {
    height: calc(var(--space) * 6);
    padding: 0 calc(var(--space) * 3);
    background: transparent;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    cursor: pointer;
  }
  .acts button:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
