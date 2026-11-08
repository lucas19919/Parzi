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
let lastCheck = 0;
let failures = 0;
let retryTimer: ReturnType<typeof setTimeout> | null = null;

/** How often a running Parzi looks for a new build. */
export const CHECK_EVERY = 30 * 60 * 1000;
/** Coming back to the window looks again if the last look is this old. */
export const FOCUS_AFTER = 10 * 60 * 1000;

/** After a failed look, when to try again: soon at first, then less often,
 *  so a launch before the network is up still finds the update. */
export function retryIn(failures: number): number {
  const minutes = [1, 3, 10, 30][Math.min(Math.max(failures, 1), 4) - 1];
  return minutes * 60 * 1000;
}

// Looks a few seconds after launch, every half hour while Parzi runs, and
// when the window comes back after a while, so the indicator appears soon
// after a build is out, not only after a look by hand.
export function checkForUpdatesSoon(ms = 12000) {
  if (scheduled) return;
  scheduled = true;
  setTimeout(() => void check(false), ms);
  setInterval(() => void check(false), CHECK_EVERY);
  window.addEventListener("focus", () => {
    if (Date.now() - lastCheck > FOCUS_AFTER) void check(false);
  });
}

/** Looks for a newer build. `force` is a look by hand: it shows what it is
 *  doing and any error. A background look stays quiet and retries on its
 *  own, and keeps a downloaded update ready unless a newer one is out. */
export async function check(force = false) {
  if (downloading) return;
  lastCheck = Date.now();
  if (retryTimer) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
  const wasReady = get(updateState) === "ready";
  if (force) updateState.set("checking");
  try {
    const u: PendingUpdate | null = await api.updateCheck(get(devChannel));
    failures = 0;
    if (!u) {
      if (force || !wasReady) {
        updateState.set("uptodate");
        updateMsg.set(get(devChannel) ? "You're on the latest dev build." : "You're on the latest version.");
      }
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
    failures += 1;
    if (force) {
      updateState.set("error");
      updateMsg.set(`Update check failed (${e}). Dev builds check nothing: install from a release to update.`);
    }
    retryTimer = setTimeout(() => void check(false), retryIn(failures));
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
    // Try again on its own: a dropped connection should not cost the update.
    failures += 1;
    retryTimer = setTimeout(() => void check(false), retryIn(failures));
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
