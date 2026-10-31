<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { tooltip } from "./tooltip";
  import { notifications, markRead, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  $: unread = $notifications.filter((n) => !n.read).length;

  interface Row {
    ids: string[];
    title: string;
    body: string;
    sessionId?: string;
    time: number;
    unread: boolean;
  }

  // The same news from the same session in a row reads as one line with a count.
  function group(list: NotificationItem[]): Row[] {
    const rows: Row[] = [];
    for (const n of list) {
      const last = rows[rows.length - 1];
      if (last && last.sessionId === n.sessionId && last.title === n.title && last.body === n.body) {
        last.ids.push(n.id);
        last.unread ||= !n.read;
      } else {
        rows.push({ ids: [n.id], title: n.title, body: n.body, sessionId: n.sessionId, time: n.time, unread: !n.read });
      }
    }
    return rows;
  }

  $: rows = group($notifications);

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return "now";
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h`;
    return `${Math.floor(h / 24)}d`;
  }

  function open(r: Row) {
    r.ids.forEach(markRead);
    if (r.sessionId) dispatch("openSession", { id: r.sessionId });
  }
</script>

{#if rows.length}
  <div class="pin">
    <InfoCard id="notifications" title="Notifications">
      <svelte:fragment slot="aside">{#if unread}{unread} new{/if}</svelte:fragment>
      <svelte:fragment slot="actions">
        <button class="icon-btn" use:tooltip={"Clear all"} aria-label="Clear all notifications" on:click={clearNotifications}>
          <Icon name="close" size={12} />
        </button>
      </svelte:fragment>
      <div class="list">
        {#each rows as r (r.ids[0])}
          <button class="item" class:unread={r.unread} on:click={() => open(r)}>
            <span class="top">
              <span class="name t-label">{r.title}</span>
              {#if r.ids.length > 1}<span class="t-hint">×{r.ids.length}</span>{/if}
              <span class="time t-hint">{timeAgo(r.time)}</span>
            </span>
            {#if r.body}<span class="body t-hint">{r.body}</span>{/if}
          </button>
        {/each}
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
    background: var(--bg);
    border-radius: var(--radius);
  }
  /* Rows run to the card's edges, divided like the rows in Settings. */
  .list {
    display: flex;
    flex-direction: column;
    max-height: calc(var(--space) * 48);
    margin: 0 calc(var(--space) * -3) calc(var(--space) * -3);
    overflow-y: auto;
    border-radius: 0 0 var(--radius) var(--radius);
  }
  .item {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    width: 100%;
    flex: none;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: none;
    border: none;
    border-top: 1px solid var(--line);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--hover);
  }
  .top {
    display: flex;
    align-items: baseline;
    gap: calc(var(--space) * 2);
    min-width: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item:not(.unread) .name {
    color: var(--muted);
    font-weight: 400;
  }
  .time {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .body {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
