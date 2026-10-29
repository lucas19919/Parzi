import type { SessionMeta } from "./api";
import { folderName } from "./tabs";

export function agentOf(s: SessionMeta): string {
  return (s.model ?? "").split("/")[0];
}

export function whereOf(s: SessionMeta): string {
  if (!s.cwd || /[\\/]\.parzi[\\/]scratch[\\/]/.test(s.cwd)) return "";
  return folderName(s.cwd);
}

let clearer: (() => Promise<void>) | null = null;

export function provideClearSessions(fn: () => Promise<void>): () => void {
  clearer = fn;
  return () => {
    if (clearer === fn) clearer = null;
  };
}

export async function clearFinishedSessions(): Promise<void> {
  if (!clearer) throw new Error("Sessions aren't ready yet");
  await clearer();
}
