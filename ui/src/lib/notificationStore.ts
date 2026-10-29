import { writable } from "svelte/store";

export interface NotificationItem {
  id: string;
  title: string;
  body: string;
  kind: "info" | "success" | "error" | "question" | "approval" | "subagent";
  sessionId?: string;
  key?: string;
  time: number;
  read: boolean;
}

const STORAGE_KEY = "parzi.notifications";
const MAX_NOTIFICATIONS = 40;

function loadInitial(): NotificationItem[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed;
    }
  } catch {}
  return [];
}

function persist(items: NotificationItem[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items.slice(0, MAX_NOTIFICATIONS)));
  } catch {}
}

export const notifications = writable<NotificationItem[]>(loadInitial());

export function addNotification(item: Omit<NotificationItem, "id" | "time" | "read">) {
  const next: NotificationItem = {
    ...item,
    id: `noti-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    time: Date.now(),
    read: false,
  };
  notifications.update((list) => {
    const updated = [next, ...list].slice(0, MAX_NOTIFICATIONS);
    persist(updated);
    return updated;
  });
}

export function markAllRead() {
  notifications.update((list) => {
    const updated = list.map((n) => ({ ...n, read: true }));
    persist(updated);
    return updated;
  });
}

export function markRead(id: string) {
  notifications.update((list) => {
    const updated = list.map((n) => (n.id === id ? { ...n, read: true } : n));
    persist(updated);
    return updated;
  });
}

export function markSessionRead(sessionId: string) {
  notifications.update((list) => {
    if (!list.some((n) => n.sessionId === sessionId && !n.read)) return list;
    const updated = list.map((n) => (n.sessionId === sessionId ? { ...n, read: true } : n));
    persist(updated);
    return updated;
  });
}

export function dropNotification(key: string) {
  notifications.update((list) => {
    if (!list.some((n) => n.key === key)) return list;
    const updated = list.filter((n) => n.key !== key);
    persist(updated);
    return updated;
  });
}

export function clearNotifications() {
  notifications.set([]);
  persist([]);
}
