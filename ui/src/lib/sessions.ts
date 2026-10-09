import type { SessionMeta } from "./api";
import { folderName } from "./tabs";

export function agentOf(s: SessionMeta): string {
  return s.model.split("/")[0];
}

export function whereOf(s: SessionMeta): string {
  if (!s.cwd || /[\\/]\.parzi[\\/]scratch[\\/]/.test(s.cwd)) return "";
  return folderName(s.cwd);
}

export function isLiveSession(s: SessionMeta, running: Set<string>): boolean {
  return running.has(s.id) || s.status === "active";
}
