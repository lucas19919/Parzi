<script lang="ts">
  // The session column's notice list. It stays at the top while the column scrolls.
  import { createEventDispatcher } from "svelte";
  import InfoCard from "./InfoCard.svelte";
  import { notifications, markAllRead, markRead, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

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
      <svelte:fragment slot="aside">{unread ? `${unread} new` : ""}</svelte:fragment>
      <div class="list">
        {#each $notifications as n (n.id)}
          <button class="item" class:unread={!n.read} on:click={() => open(n)}>
            <span class="name">{n.title}</span>
            <span class="time">{timeAgo(n.time)}</span>
            {#if n.body}<span class="body">{n.body}</span>{/if}
          </button>
        {/each}
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
  .list {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    max-height: calc(var(--space) * 48);
    overflow-y: auto;
  }
  .item {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: calc(var(--space) * 2);
    row-gap: var(--space);
    align-items: baseline;
    width: 100%;
    margin: 0;
    padding: calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .unread .name {
    font-weight: 650;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    color: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .body {
    grid-column: 1 / -1;
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
    padding: 0 calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-xs);
    cursor: pointer;
  }
  .acts button:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
