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

  interface Row {
    ids: string[];
    title: string;
    body: string;
    kind: NotificationItem["kind"];
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
        rows.push({ ids: [n.id], title: n.title, body: n.body, kind: n.kind, sessionId: n.sessionId, time: n.time, unread: !n.read });
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
      <div class="list">
        {#each rows as r (r.ids[0])}
          <button class="info-line" class:quiet={!r.unread} use:tooltip={r.body} on:click={() => open(r)}>
            <Icon name={ICONS[r.kind] ?? "bell"} size={12} />
            <span class="info-v">{r.title}</span>
            <span class="info-time">{r.ids.length > 1 ? `×${r.ids.length} · ` : ""}{timeAgo(r.time)}</span>
          </button>
        {/each}
      </div>
      <button class="btn sm" on:click={clearNotifications}>Clear</button>
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
  .list {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    max-height: calc(var(--space) * 48);
    overflow-y: auto;
  }
</style>
