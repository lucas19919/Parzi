import { test } from "node:test";
import assert from "node:assert/strict";
import { get } from "svelte/store";
import { addNotification, clearNotifications, dropNotification, newsKey, notifications } from "../src/lib/notificationStore.ts";

const news = (session: string, body: string) =>
  ({ title: "Agent", body, kind: "success", sessionId: session, key: newsKey(session) }) as const;

test("a session's news replaces its last news instead of stacking", () => {
  clearNotifications();
  addNotification(news("a", "Finished"));
  addNotification(news("a", "Stopped with an error"));
  const list = get(notifications);
  assert.equal(list.length, 1);
  assert.equal(list[0].body, "Stopped with an error");
  assert.equal(list[0].read, false);
});

test("each session keeps its own line, newest first", () => {
  clearNotifications();
  addNotification(news("a", "Finished"));
  addNotification(news("b", "Finished"));
  addNotification(news("a", "Finished"));
  assert.deepEqual(get(notifications).map((n) => n.sessionId), ["a", "b"]);
});

test("a pending ask stands apart from the session's news and goes when settled", () => {
  clearNotifications();
  addNotification(news("a", "Finished"));
  addNotification({ title: "Agent", body: "Needs your approval", kind: "approval", sessionId: "a", key: "ask-1" });
  assert.equal(get(notifications).length, 2);
  dropNotification("ask-1");
  assert.deepEqual(get(notifications).map((n) => n.key), [newsKey("a")]);
});
