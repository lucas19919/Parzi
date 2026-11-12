<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import type { IconName } from "./icons";
  import { tooltip } from "./tooltip";
  import { notifications, markRead, markAllRead, dismissNotification, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  $: unread = $notifications.filter((n) => !n.read).length;
  $: sorted = [...$notifications].sort((a, b) => Number(a.read) - Number(b.read) || b.time - a.time);

  const ICONS: Record<NotificationItem["kind"], IconName> = {
    info: "bell",
    success: "checkCircle",
    error: "xCircle",
    question: "alert",
    approval: "alert",
    subagent: "bot",
  };

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return "now";
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

<InfoCard id="notifications" title="Inbox">
  <svelte:fragment slot="aside">{#if unread}{unread} new{:else if $notifications.length}caught up{/if}</svelte:fragment>
  <svelte:fragment slot="actions">
    {#if unread}<button class="icon-btn" use:tooltip={"Mark all read"} aria-label="Mark all read" on:click={markAllRead}><Icon name="check" size={12} /></button>{/if}
  </svelte:fragment>
  {#if sorted.length}
    <div class="list" role="list">
      {#each sorted as n (n.id)}
        <div class="item" class:quiet={n.read} role="listitem">
          <button class="main" use:tooltip={n.body || n.title} on:click={() => open(n)}>
            <span class="dot {n.kind}"><Icon name={ICONS[n.kind] ?? "bell"} size={12} /></span>
            <span class="texts">
              <span class="t">{n.title}</span>
              {#if n.body}<span class="b">{n.body.slice(0, 120)}</span>{/if}
            </span>
            <span class="info-time">{timeAgo(n.time)}</span>
            {#if !n.read}<span class="unread" aria-label="Unread" />{/if}
          </button>
          {#if n.sessionId}
            <button class="go" use:tooltip={"Open session"} aria-label="Open session" on:click={() => open(n)}><Icon name="external" size={12} /></button>
          {/if}
          <button class="go" use:tooltip={"Dismiss"} aria-label="Dismiss" on:click={() => dismissNotification(n.id)}><Icon name="xCircle" size={12} /></button>
        </div>
      {/each}
    </div>
    <button class="btn sm" on:click={clearNotifications}>Clear inbox</button>
  {:else}
    <div class="empty">All caught up. Work that needs you lands here.</div>
  {/if}
</InfoCard>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: calc(var(--space) * 72);
    overflow-y: auto;
    margin: 0 calc(var(--space) * -3);
    padding: 0 calc(var(--space) * 3);
  }
  .item {
    display: flex;
    align-items: center;
    gap: var(--space);
    border-radius: var(--radius);
    padding: var(--space) var(--space);
    margin: 0 calc(var(--space) * -1);
  }
  .item:hover {
    background: var(--hover);
  }
  .item.quiet .t {
    color: var(--muted);
    font-weight: 400;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: flex-start;
    gap: calc(var(--space) * 2);
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    text-align: left;
    cursor: pointer;
    color: var(--text);
  }
  .dot {
    flex: none;
    display: inline-flex;
    margin-top: 2px;
  }
  .dot.info { color: var(--muted); }
  .dot.success { color: var(--ok-text); }
  .dot.error { color: var(--bad-text); }
  .dot.question, .dot.approval { color: var(--warn-text); }
  .dot.subagent { color: var(--accent); }
  .texts {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .info-time {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .unread {
    flex: none;
    width: 6px;
    height: 6px;
    margin-top: 5px;
    border-radius: 50%;
    background: var(--accent);
  }
  .go {
    flex: none;
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }
  .item:hover .go, .go:focus-visible {
    opacity: 1;
  }
  .go:hover {
    background: var(--line);
    color: var(--text);
  }
  .empty {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
