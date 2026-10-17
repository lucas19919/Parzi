import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DeskCmd, DeskTab } from "./desk";

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
  // How the last run ended (absent from older backends).
  last_run?: RunOutcome | null;
  // The plan item this session works on; children count for their parent's.
  plan_item?: string | null;
  // The git branch the last run ended on ("" outside a repo).
  branch?: string;
  effort?: string;
}

export type RunEnd = "completed" | "failed" | "paused" | "stopped" | "crashed" | "interrupted" | "not_started";

export interface RunOutcome {
  end: RunEnd;
  // The last assistant message, cleaned (at most 600 chars).
  summary: string;
  files: { path: string; via: "tool" | "git" }[];
  files_more: number;
  // Test and build commands, the latest result of each.
  checks: { cmd: string; ok: boolean }[];
  cost_usd: number | null;
  ms: number;
  ended: number;
}

export type ChatEvent =
  | { kind: "system"; text: string; at?: number }
  | { kind: "user"; text: string; at?: number }
  | { kind: "assistant"; text: string; done: boolean; at?: number }
  | { kind: "reasoning"; text: string; at?: number }
  | { kind: "tool_call"; id: string; name: string; args: unknown; at?: number }
  | { kind: "tool_result"; id: string; name: string; ok: boolean; output: string; ms: number; at?: number }
  | { kind: "widget"; fence: string; payload: unknown; at?: number }
  | { kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown; at?: number }
  | { kind: "error"; message: string; class: string; at?: number }
  | { kind: "checkpoint"; summary: string; at?: number };

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
  // What the call does, in words ("Running `npm test`").
  label?: string;
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

export interface AgentRequest {
  key: string;
  session: string;
  request: string;
  kind: string;
}

export type UiEvent =
  | { kind: "text"; session: string; text: string }
  | { kind: "reasoning"; session: string; text: string }
  | { kind: "tool_call"; session: string; id: string; name: string; label: string }
  | { kind: "tool_result"; session: string; id: string; name: string; ok: boolean; ms: number }
  | { kind: "notice"; session: string; text: string }
  | { kind: "context"; session: string; used: number; limit: number }
  | { kind: "approval"; key: string; session: string; call: ApprovalCall }
  | { kind: "expired"; key: string; session: string }
  | { kind: "question"; key: string; session: string; question: string; options: string[] }
  | { kind: "request"; key: string; session: string; request: string; req_kind: string }
  | { kind: "done"; session: string; turns: number }
  // The run stopped at its budget; like done for the running set.
  | { kind: "paused"; session: string; reason: string }
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
  lanes: { default_mode: string; default_allowed_tools: string[]; no_shell?: boolean };
  mcp: { servers: Record<string, unknown> };
  orchestrator: { max_concurrent: number; mcp_idle_kill_secs: number; queue_when_busy: boolean; approval_wait_secs?: number };
  routing: { order: string[] };
  budget?: { max_cost_usd: number | null; max_tokens: number | null };
  favorite_models: string[];
}

export interface Theme {
  font: { family: string; size: number; mono: string; mono_size: number };
  colors: { sidebar: string; stage: string; accent: string; text: string; text_dim: string; bar: string; border: string };
  background: {
    image: string;
    dim: number;
    vignette: number;
    blur: number;
    auto_accent?: boolean;
    // Dither strength (0-1) and tile size (px); how far down the wallpaper reaches (0.3-1).
    grain: number;
    grain_size: number;
    height: number;
  };
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
  remote?: boolean;
  // Plan items quoted into the message (at most 4).
  quotes?: { project: string; id: string }[];
  // A side task: the session it was started from, which it lists under.
  parent?: string | null;
}

export type ComposerMode = "search" | "build" | "work";

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
      remote: a.remote ?? false,
      quotes: a.quotes ?? [],
      parent: a.parent ?? null,
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
  purgeSessions: () => invoke<number>("purge_sessions"),

  pickFolder: (start?: string) => invoke<string | null>("pick_folder", { start: start || null }),
  listFiles: (root: string, query: string) => invoke<string[]>("list_files", { root, query }),
  gitBranch: (cwd: string) => invoke<string>("git_branch", { cwd }),
  gitStatus: (cwd: string) => invoke<GitStatus | null>("git_status", { cwd }),
  stageImage: (name: string, base64Data: string) => invoke<string>("stage_image", { name, base64Data }),
  readImageDataUrl: (path: string, cwd: string) => invoke<string>("read_image_data_url", { path, cwd }),

  providerStatuses: () => invoke<ProviderStatus[]>("provider_statuses"),
  refreshProviders: (ids?: string[]) => invoke<ProviderStatus[]>("refresh_providers", { ids: ids ?? null }),
  warmAgent: (provider: string) => invoke<void>("warm_agent", { provider }),
  getConfig: () => invoke<ParziConfig>("get_config"),
  saveConfig: (cfg: ParziConfig) => invoke<void>("save_config", { cfg }),
  // Starts a connector (signing in, the first time) and counts its tools.
  connectorCheck: (name: string) => invoke<number>("connector_check", { name }),
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

  // `owner`: the agent session a page belongs to. `focus` stays off: focusing
  // the page window as it appears turns the next shell click into an
  // activation click, so that click never selects. Enter in the address bar
  // calls browserFocus when the keyboard should move.
  browserShow: (
    tab: string,
    rect: { x: number; y: number; width: number; height: number },
    url: string,
    owner: string | null = null,
    focus = false,
  ) => invoke<void>("browser_show", { tab, ...rect, url, owner, focus }),
  browserHide: () => invoke<void>("browser_hide"),
  browserPrepare: (tab: string, url: string, owner: string | null = null) => invoke<void>("browser_prepare", { tab, url, owner }),
  browserFocus: (tab: string) => invoke<void>("browser_focus", { tab }),
  browserRecover: (tab: string) => invoke<void>("browser_recover", { tab }),
  browserExternal: (id: string, open: boolean) => invoke<void>("browser_external", { id, open }),
  browserPermission: (id: string, allow: boolean, remember = true) => invoke<void>("browser_permission", { id, allow, remember }),
  browserSnapshot: (tab: string) => invoke<string>("browser_snapshot", { tab }),
  searchSuggest: (query: string) => invoke<string[]>("search_suggest", { query }),
  browserClose: (tab: string) => invoke<void>("browser_close", { tab }),
  browserNavigate: (tab: string, url: string) => invoke<void>("browser_navigate", { tab, url }),
  browserNav: (tab: string, action: "back" | "forward" | "reload" | "stop") =>
    invoke<void>("browser_nav", { tab, action }),
  windowFullscreen: (on: boolean) => invoke<void>("window_fullscreen", { on }),
};

export interface GitStatus {
  branch: string;
  upstream: boolean;
  ahead: number;
  behind: number;
  changed: number;
  untracked: number;
  last_subject: string;
  last_time: number;
}

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
  /** Set on a sync conflict copy: the note it is a copy of. */
  conflict_of?: string | null;
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
  overflow: string[];
  chars: number;
  tokens: number;
}

export const EVERYWHERE = "all";

export type ObsidianState = "missing" | "unregistered" | "ready";

interface BrowserSource {
  id: string;
  name: string;
  // The profile's folder, which the import asks for.
  profile: string;
  // Its name as the browser shows it.
  label: string;
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
  // A slug gives an existing (perhaps synced) project this PC's folder.
  upsertProject: (title: string, folder: string, slug?: string) =>
    invoke<Project>("brain_project_upsert", { title, folder, slug: slug ?? null }),
  map: (note: string, project: string, on: boolean) => invoke<NoteMeta>("brain_map", { note, project, on }),
  context: (cwd: string) => invoke<BrainContext | null>("brain_context", { cwd }),
  pin: (path: string, on: boolean) => invoke<NoteMeta>("brain_pin", { path, on }),
  // Pinned notes too long to send in full: path -> project slugs ("all" outside projects).
  overflow: () => invoke<Record<string, string[]>>("brain_overflow"),
  // Keep one side of a sync conflict; resolves to the original note's path.
  resolveConflict: (path: string, keepCopy: boolean) => invoke<string>("brain_resolve_conflict", { path, keepCopy }),
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
  // false: the load failed (no network, DNS, an error page): not history.
  ok?: boolean;
  favicon?: string;
  crashed?: "browser" | "exited" | "unresponsive";
  popupBlocked?: string;
}

export interface AdblockState {
  enabled: boolean;
  allowed: boolean;
  host: string;
  supported?: boolean;
}

export interface BrowserAsk {
  kind: "permission" | "external";
  id: string;
  tab: string;
  origin?: string;
  permission?: string;
  url?: string;
}

export interface SitePermission {
  origin: string;
  kind: string;
  allow: boolean;
}

export interface BrowserPrefs {
  devtools: boolean;
  agentSignIns: boolean;
  sites: SitePermission[];
  windows: boolean;
}

export const browserPrefs = {
  get: () => invoke<BrowserPrefs>("browser_prefs"),
  set: (name: "devtools" | "agentSignIns", on: boolean) => invoke<BrowserPrefs>("browser_set_pref", { name, on }),
  forget: (origin: string, kind: string | null = null) => invoke<BrowserPrefs>("browser_forget_site", { origin, kind }),
};

export interface Download {
  id: string;
  tab: string;
  url: string;
  name: string;
  path: string;
  received: number;
  total: number;
  state: "progress" | "done" | "failed" | "canceled";
  started: number;
}

export const downloads = {
  list: () => invoke<Download[]>("browser_downloads"),
  reveal: (id: string) => invoke<void>("browser_download_reveal", { id }),
  cancel: (id: string) => invoke<void>("browser_download_cancel", { id }),
  clear: () => invoke<Download[]>("browser_downloads_clear"),
};

export function onDownload(cb: (d: Download) => void) {
  return listen<Download>("parzi://download", (ev) => cb(ev.payload));
}

export function onBrowserAsk(cb: (a: BrowserAsk) => void) {
  return listen<BrowserAsk>("parzi://browser-ask", (ev) => cb(ev.payload));
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
  install: () => invoke<void>("vault_install"),
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

export function deskSync(tabs: DeskTab[], rev: number, active: string) {
  return invoke<void>("desk_sync", { tabs, rev, active });
}

export function onDesk(cb: (cmd: DeskCmd) => void) {
  return listen<DeskCmd>("parzi://desk", (ev) => cb(ev.payload));
}

// Plans (PLAN_CONTRACT.md): one living plan per project, a Brain note the
// agents write. Item states beyond what agents declare (working, needs you,
// in review) are read live in planModel.ts.
export type PlanState = "planned" | "active" | "done" | "dropped";

export interface PlanItem {
  id: string;
  title: string;
  detail: string;
  // One of Plan.phases, or "" for the first phase.
  phase: string;
  // Ids in the same plan.
  needs: string[];
  state: PlanState;
  // "" when unknown; agents use "plan/<id>".
  branch: string;
  note: string;
}

export interface Plan {
  version: 1;
  title: string;
  goal: string;
  // In order; may be empty (one group).
  phases: string[];
  items: PlanItem[];
  updated: number;
  by: string;
}

export interface PlanView {
  project: string;
  path: string;
  // null when the project has no plan.
  plan: Plan | null;
  // The note exists but its parzi-plan block can't be read.
  error: string | null;
}

export interface PrInfo {
  number: number;
  title: string;
  url: string;
  // The head branch.
  head: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  checks: "pass" | "fail" | "pending" | "none";
}

// An item quoted into the composer; sent as { project, id }.
export interface PlanQuote {
  project: string;
  id: string;
  title: string;
}

export const planApi = {
  view: (project: string) => invoke<PlanView>("plan_view", { project }),
  // Empty when gh is missing, signed out, or the folder isn't on GitHub.
  prs: (cwd: string) => invoke<PrInfo[]>("plan_prs", { cwd }),
};

export function onPlan(cb: (e: { project: string }) => void) {
  return listen<{ project: string }>("parzi://plan", (ev) => cb(ev.payload));
}
