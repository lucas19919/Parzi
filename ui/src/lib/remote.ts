import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ChatEvent, ProviderStatus, SessionMeta } from "./api";

export interface RemoteInfo {
  label: string;
  user: string;
  host: string;
  version: string;
  linked_at: number;
  alive: boolean;
}

export interface RemoteProgress {
  step: string;
  state: "run" | "ok" | "fail" | "note";
  detail: string;
}

export interface RemoteApproval {
  key: string;
  session: string;
  name: string;
  lane: string;
  label: string;
}

export interface RemoteEvents {
  session: SessionMeta;
  events: ChatEvent[];
  from: number;
  total: number;
}

// The remote engine answers the same shapes the local one stores; only
// the ops in parzi_runtime::remote::OPS pass the desk.
export const remote = {
  info: () => invoke<RemoteInfo | null>("remote_info"),
  setup: (user: string, host: string, password: string, notes: boolean) =>
    invoke<RemoteInfo>("remote_setup", { user, host, password: password || null, notes }),
  forget: () => invoke<void>("remote_forget"),
  agent: (provider: string, action: "install" | "login") => invoke<void>("remote_agent", { provider, action }),
  sessions: () => call<{ sessions: SessionMeta[] }>("session.list").then((r) => r.sessions),
  events: (id: string, from = 0) => call<RemoteEvents>("session.events", { id, from }),
  send: (target: string, message: string, model: string, effort = "medium") =>
    call<{ id: string; status: string }>("session.send", { target, message, model, effort, cwd: "" }),
  kill: (id: string) => call<{ id: string }>("session.kill", { id }),
  rename: (id: string, title: string) => call<{ id: string }>("session.rename", { id, title }),
  remove: (id: string) => call<{ id: string }>("session.delete", { id }),
  approvals: () => call<{ approvals: RemoteApproval[] }>("approval.list").then((r) => r.approvals),
  answer: (key: string, allow: boolean) => call<{ key: string }>("approval.answer", { key, allow }),
  providers: (refresh = false) =>
    call<{ providers: ProviderStatus[] }>("providers", { refresh }).then((r) => r.providers),
};

function call<T>(op: string, body: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>("remote_call", { op, body });
}

export function onRemoteProgress(fn: (p: RemoteProgress) => void) {
  return listen<RemoteProgress>("parzi://remote-setup", (e) => fn(e.payload));
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
  brain: "Brain notes",
  service: "Service",
  linger: "Stay running",
  engine: "Engine",
  agents: "Agents",
  link: "Link",
};

export const stepName = (step: string): string => STEP_NAMES[step] ?? step;

// One row per step: a later line for the same step replaces the earlier.
export function mergeProgress(rows: RemoteProgress[], next: RemoteProgress): RemoteProgress[] {
  const at = rows.findIndex((r) => r.step === next.step);
  if (at < 0) return [...rows, next];
  const copy = rows.slice();
  copy[at] = next;
  return copy;
}
