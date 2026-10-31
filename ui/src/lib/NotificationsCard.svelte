<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import type { IconName } from "./icons";
  import { tooltip } from "./tooltip";
  import { notifications, markRead, clearNotifications, type NotificationItem } from "./notificationStore";

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  $: unread = $notifications.filter((n) => !n.read).length;

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

{#if $notifications.length}
    <InfoCard id="notifications" title="Notifications">
      <svelte:fragment slot="aside">{#if unread}{unread} new{/if}</svelte:fragment>
      <div class="list">
        {#each $notifications as n (n.id)}
          <button class="info-line" class:quiet={n.read} use:tooltip={n.body} on:click={() => open(n)}>
            <Icon name={ICONS[n.kind] ?? "bell"} size={12} />
            <span class="info-v">{n.title}</span>
            <span class="info-time">{timeAgo(n.time)}</span>
          </button>
        {/each}
      </div>
      <button class="btn sm" on:click={clearNotifications}>Clear</button>
    </InfoCard>
{/if}

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    max-height: calc(var(--space) * 48);
    overflow-y: auto;
  }
</style>
