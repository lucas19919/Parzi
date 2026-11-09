import { writable } from "svelte/store";

/** How much Parzi tells you about agents you are not looking at.
 *  - all: finished runs, problems and anything that needs you
 *  - needed: only what needs you (approvals, questions, requests) and
 *    problems; finished runs go to the Notifications card quietly
 *  - off: nothing pops up; the Notifications card still lists it all */
export type NotifyLevel = "all" | "needed" | "off";

const LEVEL_KEY = "parzi.notify.level";
const SOUND_KEY = "parzi.notify.sounds";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

const savedLevel = read(LEVEL_KEY);
export const notifyLevel = writable<NotifyLevel>(savedLevel === "all" || savedLevel === "off" ? savedLevel : "needed");
notifyLevel.subscribe((v) => write(LEVEL_KEY, v));

/** A short tone when an agent needs you and you are elsewhere. */
export const notifySounds = writable<boolean>(read(SOUND_KEY) !== "0");
notifySounds.subscribe((v) => write(SOUND_KEY, v ? "1" : "0"));

/** The same notice (digits aside: "attempt 2 of 5") from the same session
 *  shows once in ten minutes (or `within`), however often it repeats. */
const seen = new Map<string, number>();
export function freshNotice(session: string, text: string, now = Date.now(), within = 10 * 60_000): boolean {
  const key = `${session}\u0000${text.replace(/\d+/g, "#")}`;
  const at = seen.get(key);
  if (at !== undefined && now - at < within) return false;
  seen.set(key, now);
  if (seen.size > 200) seen.delete(seen.keys().next().value as string);
  return true;
}
