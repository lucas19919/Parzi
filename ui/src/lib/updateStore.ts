import { writable } from "svelte/store";
import { toast } from "./toast";

export type UpdateStatus = "idle" | "checking" | "available" | "downloading" | "ready" | "uptodate" | "error";

export const updateState = writable<UpdateStatus>("idle");
export const updateVersion = writable("");
export const updateMsg = writable("");
export const dlTotal = writable(0);
export const dlDone = writable(0);

interface PendingUpdate {
  version: string;
  body?: string;
  download: (cb?: (e: unknown) => void) => Promise<void>;
  install: () => Promise<void>;
}

let scheduled = false;
let pending: PendingUpdate | null = null;
let downloading = false;
let readyVersion = "";

const CHECK_KEY = "parzi.update.last-check";
const CHECK_EVERY = 24 * 3600 * 1000;

function lastCheck(): number {
  try {
    return Number(localStorage.getItem(CHECK_KEY)) || 0;
  } catch {
    return 0;
  }
}

function markChecked() {
  try {
    localStorage.setItem(CHECK_KEY, String(Date.now()));
  } catch {}
}

export function checkForUpdatesSoon(ms = 12000) {
  if (scheduled) return;
  scheduled = true;
  setTimeout(() => void check(false), ms);
}

export async function check(force = false) {
  if (downloading) return;
  if (!force && Date.now() - lastCheck() < CHECK_EVERY) return;
  updateState.set("checking");
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const u = (await check()) as PendingUpdate | null;
    markChecked();
    if (!u) {
      updateState.set("uptodate");
      updateMsg.set("You're on the latest version.");
      return;
    }
    if (u.version === readyVersion) {
      updateState.set("ready");
      updateMsg.set("Update downloaded — restart Parzi to use it.");
      return;
    }
    pending = u;
    updateVersion.set(u.version);
    updateState.set("available");
    updateMsg.set(`v${u.version} is available — downloading in the background…`);
    void downloadInBackground();
  } catch (e) {
    updateState.set("error");
    updateMsg.set(`Update check failed (${e}). Dev builds check nothing — install from a release to update.`);
  }
}

async function downloadInBackground() {
  if (!pending || downloading) return;
  downloading = true;
  updateState.set("downloading");
  dlTotal.set(0);
  dlDone.set(0);
  updateMsg.set("Downloading update in the background…");
  try {
    await pending.download((e: unknown) => {
      const ev = e as { event: string; data?: { contentLength?: number; chunkLength?: number } };
      if (ev.event === "Started") dlTotal.set(ev.data?.contentLength ?? 0);
      else if (ev.event === "Progress") dlDone.update((n) => n + (ev.data?.chunkLength ?? 0));
    });
    downloading = false;
    readyVersion = pending.version;
    updateState.set("ready");
    updateMsg.set("Update downloaded — restart Parzi to use it.");
    toast("Update downloaded — restart Parzi to use it");
  } catch (e) {
    downloading = false;
    updateState.set("error");
    updateMsg.set(`Download failed: ${e}`);
  }
}

export async function installUpdate() {
  if (!pending) return;
  updateState.set("downloading");
  updateMsg.set("Finishing install…");
  try {
    await pending.install();
    updateState.set("ready");
    updateMsg.set("Update installed. Restart Parzi to finish.");
    toast("Restart Parzi to finish");
  } catch (e) {
    updateState.set("error");
    updateMsg.set(`Install failed: ${e}`);
  }
}
