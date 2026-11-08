import { get, writable } from "svelte/store";
import { api } from "./api";
import { toast } from "./toast";

export type UpdateStatus = "idle" | "checking" | "available" | "downloading" | "ready" | "uptodate" | "error";

export const updateState = writable<UpdateStatus>("idle");
export const updateVersion = writable("");
export const updateMsg = writable("");
export const dlTotal = writable(0);
export const dlDone = writable(0);

/** Dev builds: pre-releases as well as releases. Opt-in, on this computer. */
const DEV_KEY = "parzi.updates.dev";
function readDev(): boolean {
  try {
    return localStorage.getItem(DEV_KEY) === "1";
  } catch {
    return false;
  }
}
export const devChannel = writable(readDev());
export function setDevChannel(on: boolean) {
  devChannel.set(on);
  try {
    localStorage.setItem(DEV_KEY, on ? "1" : "0");
  } catch {
    /* kept for this run only */
  }
  readyVersion = "";
  void check(true);
}

interface PendingUpdate {
  version: string;
  body?: string;
}

let scheduled = false;
let pending: PendingUpdate | null = null;
let downloading = false;
let readyVersion = "";

const CHECK_EVERY = 6 * 3600 * 1000;

// A found update does not survive a restart, so every launch looks again.
export function checkForUpdatesSoon(ms = 12000) {
  if (scheduled) return;
  scheduled = true;
  setTimeout(() => void check(false), ms);
  setInterval(() => void check(false), CHECK_EVERY);
}

export async function check(force = false) {
  if (downloading) return;
  if (!force && get(updateState) === "ready") return;
  updateState.set("checking");
  try {
    const u: PendingUpdate | null = await api.updateCheck(get(devChannel));
    if (!u) {
      updateState.set("uptodate");
      updateMsg.set(get(devChannel) ? "You're on the latest dev build." : "You're on the latest version.");
      return;
    }
    if (u.version === readyVersion) {
      updateState.set("ready");
      updateMsg.set("Update downloaded. Restart Parzi to use it.");
      return;
    }
    pending = u;
    updateVersion.set(u.version);
    updateState.set("available");
    updateMsg.set(`v${u.version} is available, downloading in the background…`);
    void downloadInBackground();
  } catch (e) {
    updateState.set("error");
    updateMsg.set(`Update check failed (${e}). Dev builds check nothing: install from a release to update.`);
  }
}

async function downloadInBackground() {
  if (!pending || downloading) return;
  downloading = true;
  updateState.set("downloading");
  dlTotal.set(0);
  dlDone.set(0);
  updateMsg.set("Downloading update in the background…");
  const stop = await api
    .onUpdateProgress((p) => {
      if (p.total) dlTotal.set(p.total);
      dlDone.update((n) => n + p.chunk);
    })
    .catch(() => () => {});
  try {
    await api.updateDownload();
    stop();
    downloading = false;
    readyVersion = pending.version;
    updateState.set("ready");
    updateMsg.set("Update downloaded. Restart Parzi to use it.");
  } catch (e) {
    stop();
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
    await api.updateInstall();
  } catch (e) {
    updateState.set("error");
    updateMsg.set(`Install failed: ${e}`);
    return;
  }
  // The Windows installer has closed Parzi by now; elsewhere the new app waits for a restart.
  try {
    await api.restartApp();
  } catch {
    updateState.set("ready");
    updateMsg.set("Update installed. Restart Parzi to finish.");
    toast("Restart Parzi to finish");
  }
}
