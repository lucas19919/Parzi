import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface SessionMeta {
  id: string;
  title: string;
  pinned: boolean;
  project: string;
  lane: string;
  model: string;
  status: "active" | "idle" | "done" | "killed" | "queued";
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
  context_tokens?: number;
  context_limit?: number;
  cwd: string;
  parent_id?: string | null;
  created: string;
  updated: string;
}

export type ChatEvent =
  | { kind: "system"; text: string }
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string; done: boolean }
  | { kind: "reasoning"; text: string }
  | { kind: "tool_call"; id: string; name: string; args: unknown }
  | { kind: "tool_result"; id: string; name: string; ok: boolean; output: string; ms: number }
  | { kind: "widget"; fence: string; payload: unknown }
  | { kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown }
  | { kind: "error"; message: string; class: string }
  | { kind: "checkpoint"; summary: string };

type ProviderState = "ready" | "signed_out" | "not_installed" | "disabled" | "error" | "unchecked";

interface UsageWindow {
  label: string;
  used_percent: number;
  resets_at?: number;
}

interface ProviderModel {
  id: string;
  name: string;
  is_default: boolean;
  efforts: string[];
}

export interface ProviderStatus {
  provider: string;
  state: ProviderState;
  version?: string;
  account?: string;
  hint: string;
  usage: UsageWindow[];
  models: ProviderModel[];
  gated: boolean;
  checked_at: number;
}

export interface ApprovalCall {
  id: string;
  name: string;
  args: unknown;
  lane: string;
  session: string;
}

export interface Question {
  key: string;
  session: string;
  question: string;
  options: string[];
}

export type UiEvent =
  | { kind: "text"; session: string; text: string }
  | { kind: "reasoning"; session: string; text: string }
  | { kind: "tool_call"; session: string; id: string; name: string; label: string }
  | { kind: "tool_result"; session: string; id: string; name: string; ok: boolean; ms: number }
  | { kind: "notice"; session: string; text: string }
  | { kind: "context"; session: string; used: number; limit: number }
  | { kind: "approval"; key: string; session: string; call: ApprovalCall }
  | { kind: "question"; key: string; session: string; question: string; options: string[] }
  | { kind: "done"; session: string; turns: number }
  | { kind: "error"; session: string; error: string };

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export interface ProviderEntry {
  enabled: boolean;
  binary?: string;
  default_model?: string;
}

export interface ParziConfig {
  version: number;
  providers: Record<string, ProviderEntry>;
  lanes: { default_mode: string; default_allowed_tools: string[]; max_steps: number };
  mcp: { servers: Record<string, unknown> };
  orchestrator: { max_concurrent: number; mcp_idle_kill_secs: number; queue_when_busy: boolean };
  routing: { order: string[] };
  budget?: { max_cost_usd: number | null; max_tokens: number | null };
  favorite_models: string[];
}

export interface Theme {
  font: { family: string; size: number; mono: string; mono_size: number };
  colors: { sidebar: string; stage: string; accent: string; text: string; text_dim: string; bar: string; border: string };
  background: { image: string; dim: number; vignette: number; blur: number; auto_accent?: boolean };
  glass: { opacity: number; radius: number; blur_px: number; shadow: boolean };
}

interface Palette {
  accent: string;
  average: string;
  deep: string;
}

export interface BackgroundFile {
  name: string;
  url: string;
}

export interface PackInfo {
  name: string;
  builtin: boolean;
  colors: Theme["colors"];
  has_art: boolean;
}

interface SendArgs {
  sessionId?: string | null;
  model: string;
  prompt: string;
  cwd: string;
  effort: string;
  attachments: string[];
  mode: string;
  lane: string;
}

export type ComposerMode = "search" | "build" | "work";

export const MODE_META: Record<ComposerMode, { label: string; icon: "globe" | "bot" | "brain"; tint: string; hint: string }> = {
  search: { label: "Search", icon: "globe", tint: "#6aa8ff", hint: "Search or enter an address" },
  build: { label: "Build", icon: "bot", tint: "#4e8f5c", hint: "Ask anything" },
  work: { label: "Work", icon: "brain", tint: "#e8b64c", hint: "Ask a quick question" },
};

export const api = {
  appVersion: () => invoke<string>("app_version"),
  openConfirmedUrl: (url: string) => invoke<void>("open_confirmed_url", { url }),
  openFilePath: (path: string) => invoke<void>("open_file_path", { path }),

  listThreads: () => invoke<SessionMeta[]>("list_threads"),
  getThread: (id: string) => invoke<[SessionMeta, ChatEvent[]]>("get_thread", { id }),
  sendMessage: (a: SendArgs) =>
    invoke<string>("send_message", {
      sessionId: a.sessionId ?? null,
      model: a.model,
      prompt: a.prompt,
      cwd: a.cwd,
      effort: a.effort,
      attachments: a.attachments,
      mode: a.mode,
      lane: a.lane,
    }),
  saveAnswer: (content: string) => invoke<NoteMeta>("brain_save_answer", { content }),
  renameThread: (id: string, title: string) => invoke<void>("rename_thread", { id, title }),
  deleteThread: (id: string) => invoke<number>("delete_thread", { id }),
  forkThread: (id: string) => invoke<SessionMeta>("fork_thread", { id }),
  compactThread: (id: string) => invoke<string>("compact_thread", { id }),
  killRun: (id: string) => invoke<void>("kill_run", { id }),
  approveTool: (key: string, session: string, allow: boolean) =>
    invoke<void>("approve_tool", { key, session, allow }),
  answerQuestion: (key: string, session: string, answer: string) =>
    invoke<void>("answer_question", { key, session, answer }),
  spawnTrack: (parent: string, title: string, prompt: string) =>
    invoke<string>("spawn_track", { parent, title, prompt }),
  planGet: (id: string) => invoke<string>("plan_get", { id }),
  purgeSessions: () => invoke<number>("purge_sessions"),

  pickFolder: (start?: string) => invoke<string | null>("pick_folder", { start: start || null }),
  listFiles: (root: string, query: string) => invoke<string[]>("list_files", { root, query }),
  gitBranch: (cwd: string) => invoke<string>("git_branch", { cwd }),
  stageImage: (name: string, base64Data: string) => invoke<string>("stage_image", { name, base64Data }),
  readImageDataUrl: (path: string, cwd: string) => invoke<string>("read_image_data_url", { path, cwd }),

  providerStatuses: () => invoke<ProviderStatus[]>("provider_statuses"),
  refreshProviders: (ids?: string[]) => invoke<ProviderStatus[]>("refresh_providers", { ids: ids ?? null }),
  toggleFavorite: (spec: string) => invoke<string[]>("toggle_favorite", { spec }),
  warmAgent: (provider: string) => invoke<void>("warm_agent", { provider }),
  getConfig: () => invoke<ParziConfig>("get_config"),
  saveConfig: (cfg: ParziConfig) => invoke<void>("save_config", { cfg }),
  runDoctorQuick: () => invoke<Check[]>("run_doctor_quick"),

  getTheme: () => invoke<Theme>("get_theme"),
  getThemeCss: () => invoke<string>("get_theme_css"),
  saveTheme: (theme: Theme) => invoke<void>("save_theme", { theme }),
  resetTheme: () => invoke<void>("reset_theme"),
  listPackInfos: () => invoke<PackInfo[]>("list_pack_infos"),
  savePack: (name: string) => invoke<void>("save_pack", { name }),
  applyPack: (name: string) => invoke<string>("apply_pack", { name }),
  renamePack: (old: string, name: string) => invoke<void>("rename_pack", { old, new: name }),
  deletePack: (name: string) => invoke<void>("delete_pack", { name }),
  backgroundUrl: async () => {
    const abs = await invoke<string>("background_url");
    return abs ? convertFileSrc(abs) : "";
  },
  listBackgroundUrls: () => invoke<BackgroundFile[]>("list_background_urls"),
  setBackground: (name: string) => invoke<string>("set_background", { name }),
  saveBackgroundData: (name: string, base64Data: string) =>
    invoke<string>("save_background_data", { name, base64Data }),
  deleteBackground: (name: string) => invoke<string>("delete_background", { name }),
  paletteFromBackground: () => invoke<Palette>("palette_from_background"),

  browserShow: (tab: string, rect: { x: number; y: number; width: number; height: number }, url: string) =>
    invoke<void>("browser_show", { tab, ...rect, url }),
  browserHide: () => invoke<void>("browser_hide"),
  browserPrepare: (tab: string, url: string) => invoke<void>("browser_prepare", { tab, url }),
  browserSnapshot: (tab: string) => invoke<string>("browser_snapshot", { tab }),
  searchSuggest: (query: string) => invoke<string[]>("search_suggest", { query }),
  browserClose: (tab: string) => invoke<void>("browser_close", { tab }),
  browserNavigate: (tab: string, url: string) => invoke<void>("browser_navigate", { tab, url }),
  browserNav: (tab: string, action: "back" | "forward" | "reload" | "stop") =>
    invoke<void>("browser_nav", { tab, action }),
  windowFullscreen: (on: boolean) => invoke<void>("window_fullscreen", { on }),
};

export interface NoteMeta {
  path: string;
  title: string;
  projects: string[];
  tags: string[];
  folder?: string | null;
  source?: string | null;
  pinned: boolean;
  links: string[];
  modified: number;
  bytes: number;
  summary: string;
}

export interface Project {
  slug: string;
  title: string;
  folder: string;
  note: string;
  notes: string[];
}

interface BrainContext {
  project: Project | null;
  text: string;
  attached: string[];
  listed: string[];
  chars: number;
  tokens: number;
}

export const EVERYWHERE = "all";

export type ObsidianState = "missing" | "unregistered" | "ready";

interface BrowserSource {
  id: string;
  name: string;
  profile: string;
  bookmarks: number;
  history: boolean;
}

interface NoteCandidate {
  source: string;
  title: string;
  kind: "instructions" | "memory" | "skill";
  project_folder?: string | null;
  bytes: number;
}

interface ToolSource {
  id: string;
  name: string;
  found: boolean;
  notes: NoteCandidate[];
}

interface FolderCandidate {
  path: string;
  name: string;
  sources: string[];
  last_used?: number | null;
  exists: boolean;
}

export interface Scan {
  browsers: BrowserSource[];
  tools: ToolSource[];
  folders: FolderCandidate[];
}

interface BrowserData {
  bookmarks: { title: string; url: string; folder: string }[];
  history: { url: string; title: string; visits: number; last_visit: number }[];
}

interface ImportReport {
  notes: number;
  projects: number;
  skipped: string[];
}

export const brain = {
  dir: () => invoke<string>("brain_dir"),
  list: () => invoke<NoteMeta[]>("brain_list"),
  read: (path: string) => invoke<string>("brain_read", { path }),
  write: (path: string, content: string) => invoke<NoteMeta>("brain_write", { path, content }),
  remove: (path: string) => invoke<void>("brain_delete", { path }),
  projects: () => invoke<Project[]>("brain_projects"),
  upsertProject: (title: string, folder: string) => invoke<Project>("brain_project_upsert", { title, folder }),
  map: (note: string, project: string, on: boolean) => invoke<NoteMeta>("brain_map", { note, project, on }),
  context: (cwd: string) => invoke<BrainContext | null>("brain_context", { cwd }),
  pin: (path: string, on: boolean) => invoke<NoteMeta>("brain_pin", { path, on }),
  open: (path: string, target: "file" | "folder" | "obsidian") => invoke<void>("brain_open", { path, target }),
  obsidian: () => invoke<ObsidianState>("brain_obsidian"),
};

export const onboard = {
  scan: () => invoke<Scan>("onboard_scan"),
  browser: (id: string, profile: string) => invoke<BrowserData>("onboard_browser", { id, profile }),
  importTools: (notes: string[], folders: string[]) => invoke<ImportReport>("onboard_import", { notes, folders }),
  install: (provider: string) => invoke<void>("agent_install", { provider }),
  login: (provider: string) => invoke<boolean>("agent_login", { provider }),
};

export interface PageEvent {
  tab: string;
  url?: string;
  title?: string;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  fullscreen?: boolean;
  blocked?: number;
  bg?: string;
}

export interface AdblockState {
  enabled: boolean;
  allowed: boolean;
  host: string;
}

export interface VaultState {
  status: "missing" | "unauthenticated" | "locked" | "unlocked" | string;
  email: string;
}

export interface VaultLogin {
  id: string;
  name: string;
  username: string;
}

export const vault = {
  state: () => invoke<VaultState>("vault_state"),
  unlock: () => invoke<VaultState>("vault_unlock"),
  lock: () => invoke<VaultState>("vault_lock"),
  logins: (tab: string) => invoke<{ status: string; host: string; items: VaultLogin[] }>("vault_logins", { tab }),
  fill: (tab: string, id: string) => invoke<string>("vault_fill", { tab, id }),
};

export const BITWARDEN_INSTALL = "winget install Bitwarden.CLI";

export const adblock = {
  state: (url: string) => invoke<AdblockState>("adblock_state", { url }),
  enable: (on: boolean) => invoke<AdblockState>("adblock_enable", { on }),
  site: (url: string, allow: boolean) => invoke<AdblockState>("adblock_site", { url, allow }),
};

export function onBrowserOpen(cb: (e: { tab: string; url: string }) => void) {
  return listen<{ tab: string; url: string }>("parzi://browser-open", (ev) => cb(ev.payload));
}

export function onBrowserKey(cb: (key: string) => void) {
  return listen<{ key: string }>("parzi://browser-key", (ev) => cb(ev.payload.key));
}

export function onRunEvent(cb: (e: UiEvent) => void) {
  return listen<UiEvent>("parzi://run-event", (ev) => cb(ev.payload));
}

export function onProviders(cb: (board: ProviderStatus[]) => void) {
  return listen<ProviderStatus[]>("parzi://providers", (ev) => cb(ev.payload));
}

export interface SignInStep {
  provider: string;
  step: "starting" | "browser";
}

export function onSignIn(cb: (s: SignInStep) => void) {
  return listen<SignInStep>("parzi://sign-in", (ev) => cb(ev.payload));
}

export function onBrowser(cb: (page: PageEvent) => void) {
  return listen<PageEvent>("parzi://browser", (ev) => cb(ev.payload));
}

interface DeskTab {
  id: string;
  kind: "harness" | "browser" | "brain" | "history";
  title: string;
  url: string;
  session_id: string;
}

interface DeskCmd {
  op: "open" | "focus" | "close" | "navigate";
  id?: string;
  url?: string;
  rev?: number;
  owner?: string | null;
}

export function deskSync(tabs: DeskTab[], rev: number, active: string) {
  return invoke<void>("desk_sync", { tabs, rev, active });
}

export function onDesk(cb: (cmd: DeskCmd) => void) {
  return listen<DeskCmd>("parzi://desk", (ev) => cb(ev.payload));
}
