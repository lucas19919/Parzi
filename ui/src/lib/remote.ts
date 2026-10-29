import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writable } from "svelte/store";
import type { Project, ProviderStatus } from "./api";

export interface RemoteInfo {
  label: string;
  user: string;
  host: string;
  version: string;
  linked_at: number;
  alive: boolean;
  synced_at: number;
  device: string;
}

export interface RemoteProgress {
  step: string;
  state: "run" | "ok" | "fail" | "note";
  detail: string;
}

export interface RemoteStatus {
  state: "up" | "down" | "updating" | "ready" | "error" | "resync" | "synced" | "settings" | "brain";
  detail: string;
}

export const isRemote = (id: string | null | undefined): boolean => !!id && (id.startsWith("r:") || id.startsWith("m:"));

export const remoteInfo = writable<RemoteInfo | null>(null);
export const remoteBoard = writable<ProviderStatus[]>([]);

export const remote = {
  info: () => invoke<RemoteInfo | null>("remote_info"),
  connect: () => invoke<RemoteInfo>("remote_connect"),
  setup: (user: string, host: string, password: string, notes: boolean) =>
    invoke<RemoteInfo>("remote_setup", { user, host, password: password || null, notes }),
  forget: () => invoke<void>("remote_forget"),
  agent: (provider: string, action: "install" | "login") => invoke<void>("remote_agent", { provider, action }),
  providers: (refresh = false) => invoke<ProviderStatus[]>("remote_providers", { refresh }),
  projects: () => invoke<Project[]>("remote_projects"),
  projectFolder: (slug: string | null, title: string, folder: string) =>
    invoke<Project>("remote_project_folder", { slug, title, folder }),
  sync: () => invoke<string>("remote_sync"),
};

export async function loadRemoteInfo(): Promise<RemoteInfo | null> {
  const info = await remote.info();
  remoteInfo.set(info);
  return info;
}

export async function refreshRemoteBoard(refresh = false): Promise<ProviderStatus[]> {
  const got = await remote.providers(refresh);
  const all = Array.isArray(got) ? got : [];
  remoteBoard.set(all);
  return all;
}

export function onRemoteProgress(fn: (p: RemoteProgress) => void) {
  return listen<RemoteProgress>("parzi://remote-setup", (e) => fn(e.payload));
}

export function onRemoteStatus(fn: (s: RemoteStatus) => void) {
  return listen<RemoteStatus>("parzi://remote", (e) => fn(e.payload));
}

const STEP_NAMES: Record<string, string> = {
  connect: "Connect",
  key: "Key login",
  probe: "Check the machine",
  install: "Install Parzi",
  spec: "Send settings",
  setup: "Set up",
  home: "Parzi folder",
  config: "Settings",
  brain: "Project notes",
  service: "Service",
  linger: "Stay running",
  engine: "Engine",
  agents: "Agents",
  link: "Link",
};

export const stepName = (step: string): string => STEP_NAMES[step] ?? step;

export function mergeProgress(rows: RemoteProgress[], next: RemoteProgress): RemoteProgress[] {
  const at = rows.findIndex((r) => r.step === next.step);
  if (at < 0) return [...rows, next];
  const copy = rows.slice();
  copy[at] = next;
  return copy;
}
