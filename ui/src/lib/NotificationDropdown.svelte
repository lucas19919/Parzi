<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import { notifications, markAllRead, markRead, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    close: void;
  }>();

  $: unreadCount = $notifications.filter((n) => !n.read).length;

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return "just now";
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  function iconFor(kind: NotificationItem["kind"]): IconName {
    switch (kind) {
      case "subagent":
        return "bot";
      case "question":
        return "chat";
      case "approval":
        return "shield";
      case "success":
        return "check";
      case "error":
        return "alert";
      default:
        return "spark";
    }
  }

  function clickItem(n: NotificationItem) {
    markRead(n.id);
    if (n.sessionId) {
      dispatch("openSession", { id: n.sessionId });
      dispatch("close");
    }
  }
</script>

<div class="noti-popover" role="dialog" aria-label="Notifications" data-ui-list>
  <div class="noti-header">
    <div class="title-row">
      <span class="title">Notifications</span>
      {#if unreadCount > 0}
        <span class="badge">{unreadCount} new</span>
      {/if}
    </div>
    <div class="actions">
      {#if unreadCount > 0}
        <button class="text-btn" on:click={markAllRead}>Mark read</button>
      {/if}
      {#if $notifications.length > 0}
        <button class="text-btn danger" on:click={clearNotifications}>Clear</button>
      {/if}
    </div>
  </div>

  <div class="noti-list">
    {#if $notifications.length === 0}
      <div class="empty">
        <Icon name="bell" />
        <span>No notifications yet</span>
      </div>
    {:else}
      {#each $notifications as n (n.id)}
        <button
          class="noti-item"
          class:unread={!n.read}
          class:clickable={!!n.sessionId}
          on:click={() => clickItem(n)}
        >
          <div class="icon-wrap {n.kind}">
            <Icon name={iconFor(n.kind)} size={16} />
          </div>
          <div class="body-wrap">
            <div class="top-row">
              <span class="item-title">{n.title}</span>
              <span class="time">{timeAgo(n.time)}</span>
            </div>
            <p class="body-text">{n.body}</p>
          </div>
          {#if !n.read}
            <span class="unread-dot" />
          {/if}
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .noti-popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    width: 320px;
    max-height: 440px;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    z-index: 500;
    overflow: hidden;
    user-select: none;
    animation: popIn 140ms ease-out;
  }
  @keyframes popIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .noti-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--panel) 92%, var(--text));
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .title {
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text);
  }
  .badge {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .text-btn {
    background: none;
    border: none;
    padding: 2px 4px;
    font-size: var(--fs-xs);
    color: var(--muted);
    cursor: pointer;
    border-radius: var(--radius-sm);
  }
  .text-btn:hover {
    color: var(--text);
    background: var(--hover);
  }
  .text-btn.danger:hover {
    color: var(--bad);
  }
  .noti-list {
    flex: 1;
    overflow-y: auto;
    max-height: 380px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 40px 16px;
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .noti-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    background: transparent;
    border: none;
    border-bottom: 1px solid color-mix(in srgb, var(--line) 40%, transparent);
    text-align: left;
    position: relative;
    cursor: default;
  }
  .noti-item.clickable {
    cursor: pointer;
  }
  .noti-item:hover.clickable {
    background: var(--hover);
  }
  .noti-item.unread {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }
  .icon-wrap {
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--line);
    color: var(--muted);
    margin-top: 1px;
  }
  .icon-wrap.subagent {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    color: var(--accent);
  }
  .icon-wrap.success {
    background: color-mix(in srgb, var(--success, #22c55e) 20%, transparent);
    color: var(--success, #22c55e);
  }
  .icon-wrap.error {
    background: color-mix(in srgb, var(--bad) 20%, transparent);
    color: var(--bad);
  }
  .icon-wrap.question,
  .icon-wrap.approval {
    background: color-mix(in srgb, var(--warn) 20%, transparent);
    color: var(--warn);
  }
  .body-wrap {
    flex: 1;
    min-width: 0;
  }
  .top-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 6px;
    margin-bottom: 2px;
  }
  .item-title {
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    font-size: 10px;
    color: var(--muted);
    flex: none;
  }
  .body-text {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--muted);
    line-height: 1.35;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .unread-dot {
    position: absolute;
    top: 12px;
    right: 8px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
</style>
