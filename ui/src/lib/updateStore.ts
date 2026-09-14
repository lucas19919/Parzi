import { get, writable } from "svelte/store";

/** Shared updater state: the sidebar footer badge and Settings read this. */
export type FooterUpdateState = "idle" | "checking" | "available" | "uptodate" | "error";

export const footerUpdateState = writable<FooterUpdateState>("idle");
export const footerUpdateVersion = writable("");

let scheduled: ReturnType<typeof setTimeout> | null = null;
let running = false;

/** One update check. Silent failures settle back to idle (footer stays clean);
 * an available update sticks until installed. Concurrent calls collapse. */
export async function checkForUpdates(silent = true): Promise<boolean> {
  if (running) return false;
  const current = get(footerUpdateState);
  if (current === "available" || current === "checking") return current === "available";
  running = true;
  footerUpdateState.set("checking");
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const u = await check();
    if (u) {
      footerUpdateVersion.set(u.version);
      footerUpdateState.set("available");
      return true;
    }
    footerUpdateState.set("uptodate");
    return false;
  } catch {
    footerUpdateState.set(silent ? "idle" : "error");
    return false;
  } finally {
    running = false;
  }
}

/** Boot-time check, delayed so first paint never waits on the network. */
export function checkForUpdatesSoon(ms = 12000) {
  if (!scheduled) scheduled = setTimeout(() => void checkForUpdates(true), ms);
}
