import { writable } from "svelte/store";

export const updateVersion = writable("");

let scheduled = false;

export function checkForUpdatesSoon(ms = 12000) {
  if (scheduled) return;
  scheduled = true;
  setTimeout(async () => {
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) updateVersion.set(update.version);
    } catch {}
  }, ms);
}
