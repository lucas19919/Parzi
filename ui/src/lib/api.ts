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
  /** How full the context window is: tokens of the latest request + reply. */
  context_tokens?: number;
  /** Window of the model that answered; 0/absent = not measured yet. */
  context_limit?: number;
  cwd: string;
  /** Hierarchy link for teamwork subsessions. Absent/null = top-level. */
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
  | { kind: "route_transition"; from_provider: string; to_provider: string; reason: string; cooldown_secs?: number | null }
  | { kind: "checkpoint"; summary: string };

export interface ModelInfo {
  id: string;
  name: string;
  context_limit: number;
  output_limit: number;
  price_in: number;
  price_out: number;
  tools: boolean;
  vision: boolean;
  legacy: boolean;
  is_default: boolean;
  family: string;
  family_name: string;
  variant: string | null;
}

export type Billing = "subscription" | "api_key" | "none";

export interface ModelRow {
  provider: string;
  auth: "ok" | "missing" | "expired";
  /** Class of the credential that won: a plan, a pay-per-token key, or nothing. */
  billing: Billing;
  /** Sign-in hint when missing/expired; empty when ok. */
  hint: string;
  /** Plan label when signed in ("Claude Max", "ChatGPT / Codex"). */
  account?: string | null;
  /** Settings may store an API key for this provider. */
  takes_key: boolean;
  models: ModelInfo[];
}

export type UiEvent =
  | { kind: "text"; session: string; text: string }
  | { kind: "reasoning"; session: string; text: string }
  | { kind: "tool_call"; session: string; id: string; name: string; label: string }
  | { kind: "tool_result"; session: string; id: string; name: string; ok: boolean; ms: number }
  | { kind: "notice"; session: string; text: string }
  | { kind: "usage"; session: string; tokens_in: number; tokens_out: number; cost_usd: number }
  | { kind: "context"; session: string; used: number; limit: number }
  | { kind: "approval"; key: string; call: { id: string; name: string; args: unknown; lane: string } }
  | { kind: "subsession_created"; parent_id: string; subsession: SessionMeta }
  | { kind: "done"; session: string; turns: number }
  | { kind: "error"; session: string; error: string };

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export interface McpToolView {
  name: string;
  qualified: string;
  description: string;
  exposed: boolean;
  mode: string | null;
}

export interface McpServerTools {
  server: string;
  ok: boolean;
  error: string | null;
  tools: McpToolView[];
}

export interface PluginView {
  name: string;
  version: string;
  kind: string;
  enabled: boolean;
  /** Slash-command count for `commands` packs (0 otherwise). */
  commands: number;
}

/** One slash command inside a `commands` skill pack. */
export interface SkillCommand {
  name: string;
  description: string;
  prompt: string;
}

/** What paste-installing one skill produced. */
export interface InstalledSkill {
  name: string;
  commands: number;
}

/** Result of pulling a skill library: installed names + "name — reason" skips. */
export interface SkillInstallReport {
  installed: string[];
  skipped: string[];
}

export interface EffortOption {
  id: string;
  label: string;
  hint: string;
}

export interface ParziConfig {
  version: number;
  default_provider: string;
  providers: Record<string, { default_model: string; base_url?: string }>;
  lanes: { default_mode: string; default_allowed_tools: string[]; max_steps: number };
  mcp: { servers: Record<string, unknown> };
  orchestrator: { max_concurrent: number; mcp_idle_kill_secs: number; queue_when_busy: boolean };
  routing: { auto_failover: boolean; auto_order: string[]; keys_in_auto: boolean };
  budget?: { max_cost_usd: number | null; max_tokens: number | null };
  catalog_refresh: boolean;
  favorite_models: string[];
}

export interface Theme {
  font: { family: string; size: number; mono: string; mono_size: number };
  colors: { sidebar: string; stage: string; accent: string; text: string; text_dim: string; bar: string; border: string };
  background: { image: string; dim: number; vignette: number; blur: number; auto_accent?: boolean };
  glass: { opacity: number; radius: number; blur_px: number; shadow: boolean };
}

export interface Palette {
  accent: string;
  average: string;
  deep: string;
}

export interface BackgroundFile {
  name: string;
  url: string;
}

/** A saved theme pack with its real colours (no hardcoded preview table). */
export interface PackInfo {
  name: string;
  builtin: boolean;
  colors: Theme["colors"];
  has_art: boolean;
}

export interface LaneView {
  name: string;
  mode: string;
  model: string | null;
  root: string | null;
  allowed_tools: string[];
  isolated_worktree?: boolean;
}

export interface AgentRoleConfig {
  model?: string | null;
  effort?: string | null;
  system_prompt?: string | null;
  temperature?: number | null;
}

export interface ProjectRoster {
  header: AgentRoleConfig;
  orchestrator: AgentRoleConfig;
  implementation: AgentRoleConfig;
}

export interface PlanItem {
  title: string;
  status: "Pending" | "InProgress" | "Done";
  lane?: string | null;
  line: number;
}

export interface CheckpointView {
  session_id: string;
  turn: number;
  hash: string;
  created: string;
}

/** One built-in (non-connector) agent tool with its settings group. */
export interface BuiltinTool {
  name: string;
  group: string;
  blurb: string;
}

export interface ProjectView {
  name: string;
  root: string | null;
  has_system: boolean;
  lanes: LaneView[];
}

/* ---------- Inspector deck (right bar) ---------- */

/** Normalised `ui.show_artifact` payload as shown in the Docs tab. */
export interface InspectorArtifact {
  id: string;
  title: string;
  kind: string;
  language: string;
  content: string;
  version: number;
}

/** A markdown document opened in the Docs tab (project file or transcript). */
export interface InspectorDoc {
  title: string;
  content: string;
  path?: string;
}

/** Quick-tab candidate returned by `list_project_docs`. */
export interface DocEntry {
  label: string;
  path: string;
  source: "system" | "root";
}

/** One agent in the swarm graph (root thread or subsession). */
export interface SwarmNode {
  id: string;
  title: string;
  lane: string;
  model: string;
  status: SessionMeta["status"];
  tokens: number;
  cost: number;
  parentId: string | null;
  depth: number;
  /** Last tool this session called, when known. */
  tool?: { name: string; since: number; running: boolean } | null;
}

export const api = {
  appVersion: () => invoke<string>("app_version"),
  windowMinimize: () => invoke<void>("window_minimize"),
  windowMaximize: () => invoke<boolean>("window_maximize"),
  windowClose: () => invoke<void>("window_close"),
  windowStartDragging: () => invoke<void>("window_start_dragging"),
  loginAntigravity: () => invoke<string>("login_antigravity"),
  logoutAntigravity: () => invoke<void>("logout_antigravity"),
  migrateTasks: (project: string) => invoke<number>("migrate_tasks", { project }),
  listThreads: () => invoke<SessionMeta[]>("list_threads"),
  getThread: (id: string) =>
    invoke<[SessionMeta, ChatEvent[], string]>("get_thread", { id }),
  sendMessage: (args: {
    sessionId?: string;
    project: string;
    lane: string;
    model: string;
    prompt: string;
    cwd: string;
    effort?: string;
    attachments?: string[];
    parentId?: string;
  }) =>
    invoke<string>("send_message", {
      sessionId: args.sessionId ?? null,
      project: args.project,
      lane: args.lane,
      model: args.model,
      prompt: args.prompt,
      cwd: args.cwd,
      effort: args.effort ?? null,
      attachments: args.attachments ?? [],
      parentId: args.parentId ?? null,
    }),
  createSubsession: (args: {
    parentId: string;
    title?: string;
    prompt?: string;
    model?: string;
  }) =>
    invoke<SessionMeta>("create_subsession", {
      parentId: args.parentId,
      title: args.title ?? "",
      prompt: args.prompt ?? null,
      model: args.model ?? null,
    }),
  reparentThread: (id: string, parentId?: string) =>
    invoke<void>("reparent_thread", { id, parentId: parentId ?? null }),
  renameThread: (id: string, title: string) =>
    invoke<void>("rename_thread", { id, title }),
  deleteThread: (id: string) => invoke<number>("delete_thread", { id }),
  listProjects: () => invoke<ProjectView[]>("list_projects"),
  getProjectRoster: (project: string) =>
    invoke<ProjectRoster>("get_project_roster", { project }),
  saveProjectRoster: (project: string, roster: ProjectRoster) =>
    invoke<void>("save_project_roster", { project, roster }),
  getProjectPlan: (project: string) =>
    invoke<string>("get_project_plan", { project }),
  saveProjectPlan: (project: string, content: string) =>
    invoke<void>("save_project_plan", { project, content }),
  getProjectKnowledge: (project: string) =>
    invoke<string>("get_project_knowledge", { project }),
  saveProjectKnowledge: (project: string, content: string) =>
    invoke<void>("save_project_knowledge", { project, content }),
  getWorktreeDiff: (project: string, lane: string, sessionId: string) =>
    invoke<string>("get_worktree_diff", { project, lane, sessionId }),
  applyWorktree: (project: string, lane: string, sessionId: string) =>
    invoke<string>("apply_worktree", { project, lane, sessionId }),
  listCheckpoints: (repo: string, session_id: string) =>
    invoke<CheckpointView[]>("list_checkpoints", { repo, sessionId: session_id }),
  restoreCheckpoint: (repo: string, session_id: string, turn: number) =>
    invoke<void>("restore_checkpoint", { repo, sessionId: session_id, turn }),
  listBuiltinTools: () => invoke<BuiltinTool[]>("list_builtin_tools"),
  listFiles: (root: string, query: string) =>
    invoke<string[]>("list_files", { root, query }),
  createProject: (name: string, root: string) =>
    invoke<void>("create_project", { name, root }),
  deleteProject: (name: string) => invoke<number>("delete_project", { name }),
  gitBranch: (cwd: string) => invoke<string>("git_branch", { cwd }),
  togglePin: (id: string, pinned: boolean) =>
    invoke<void>("toggle_pin", { id, pinned }),
  killRun: (id: string) => invoke<void>("kill_run", { id }),
  /** Summarize a thread into a checkpoint; returns the summary. */
  compactThread: (id: string, focus?: string) =>
    invoke<string>("compact_thread", { id, focus: focus ?? null }),
  forkThread: (id: string, at?: number) =>
    invoke<SessionMeta>("fork_thread", { id, at: at ?? null }),
  approveTool: (key: string, allow: boolean) =>
    invoke<void>("approve_tool", { key, allow }),
  getModels: (refresh?: boolean) =>
    invoke<ModelRow[]>("get_models", { refresh: refresh ?? null }),
  /** Live catalog for one provider (pick a provider, then its models). */
  refreshProvider: (provider: string) =>
    invoke<ModelRow>("refresh_provider", { provider }),
  saveKey: (provider: string, value: string) =>
    invoke<void>("save_key", { provider, value }),
  deleteKey: (provider: string) => invoke<void>("delete_key", { provider }),
  toggleFavorite: (spec: string) => invoke<string[]>("toggle_favorite", { spec }),
  effortOptions: (provider: string) => invoke<EffortOption[]>("effort_options", { provider }),
  getThemeCss: () => invoke<string>("get_theme_css"),
  getTheme: () => invoke<Theme>("get_theme"),
  resetTheme: () => invoke<void>("reset_theme"),
  purgeSessions: () => invoke<number>("purge_sessions"),
  getConfig: () => invoke<ParziConfig>("get_config"),
  saveConfig: (cfg: ParziConfig) => invoke<void>("save_config", { cfg }),
  saveTheme: (theme: Theme) => invoke<void>("save_theme", { theme }),
  backgroundUrl: async () => {
    const abs = await invoke<string>("background_url");
    return abs ? convertFileSrc(abs) : "";
  },
  runDoctor: () => invoke<Check[]>("run_doctor"),
  runDoctorQuick: () => invoke<Check[]>("run_doctor_quick"),
  runDoctorMcp: () => invoke<Check[]>("run_doctor_mcp"),
  listMcpTools: (server: string) =>
    invoke<McpServerTools>("list_mcp_tools", { server }),
  listAllMcpTools: () => invoke<McpServerTools[]>("list_all_mcp_tools"),
  listPacks: () => invoke<string[]>("list_packs"),
  listPackInfos: () => invoke<PackInfo[]>("list_pack_infos"),
  deletePack: (name: string) => invoke<void>("delete_pack", { name }),
  renamePack: (old: string, name: string) => invoke<void>("rename_pack", { old, new: name }),
  deleteBackground: (name: string) => invoke<string>("delete_background", { name }),
  getUserCss: () => invoke<string>("get_user_css"),
  saveUserCss: (css: string) => invoke<string>("save_user_css", { css }),
  savePack: (name: string) => invoke<void>("save_pack", { name }),
  applyPack: (name: string) => invoke<string>("apply_pack", { name }),
  listBackgrounds: () => invoke<string[]>("list_backgrounds"),
  listBackgroundUrls: () => invoke<BackgroundFile[]>("list_background_urls"),
  setBackground: (name: string) => invoke<string>("set_background", { name }),
  uploadBackground: (src: string) => invoke<string>("upload_background", { src }),
  /** Stores the picked picture (shrunk when oversized) and makes it the
   *  wallpaper; resolves to the name it was saved under. Tauri maps Rust's
   *  `base64_data` to the camelCase key — snake_case here was rejected. */
  saveBackgroundData: (name: string, base64Data: string) =>
    invoke<string>("save_background_data", { name, base64Data }),
  backgroundFile: async (name: string) => {
    const abs = await invoke<string>("background_file", { name });
    return convertFileSrc(abs);
  },
  paletteFromBackground: (name?: string) =>
    invoke<Palette>("palette_from_background", { name: name ?? null }),
  listPlugins: () => invoke<PluginView[]>("list_plugins"),
  togglePlugin: (name: string, enabled: boolean) =>
    invoke<void>("toggle_plugin", { name, enabled }),
  installPastedSkill: (pack_name: string, text: string) =>
    invoke<InstalledSkill>("install_pasted_skill", { packName: pack_name, text }),
  installSkillFromGit: (url: string) =>
    invoke<SkillInstallReport>("install_skill_from_git", { url }),
  deleteSkill: (name: string) => invoke<void>("delete_skill", { name }),
  openExternalUrl: (url: string) => invoke<void>("open_external_url", { url }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  writeTextFile: (path: string, content: string) =>
    invoke<void>("write_text_file", { path, content }),
  /** Stage pasted/dropped image bytes; returns an attachable absolute path. */
  stageImage: (name: string, base64Data: string) =>
    invoke<string>("stage_image", { name, base64Data }),
  /** Image bytes as a data URL for previews (confined, images only). */
  readImageDataUrl: (path: string, cwd: string) =>
    invoke<string>("read_image_data_url", { path, cwd }),
  saveProjectSystem: (project: string, content: string) =>
    invoke<string>("save_project_system", { project, content }),
  createSkill: (name: string) => invoke<void>("create_skill", { name }),
  skillCommands: (name: string) => invoke<SkillCommand[]>("skill_commands", { name }),
  saveSkillCommands: (name: string, commands: SkillCommand[]) =>
    invoke<void>("save_skill_commands", { name, commands }),
  listProjectDocs: (project: string, root: string) =>
    invoke<DocEntry[]>("list_project_docs", { project, root }),
};

export function onRunEvent(cb: (e: UiEvent) => void) {
  return listen<UiEvent>("parzi://run-event", (ev) => cb(ev.payload));
}

/* ---------- hub (workspaces, GitHub, project creation — PLAN.md §1, §2) ---------- */

// Lowercase on the wire: `workspace.toml` is meant to be read and edited by a
// person, so core serialises these enums the way they are written in the file.
export type WorkspaceKind = "solo" | "team";
export type MemberRole = "owner" | "maintainer" | "member" | "viewer";

/** One repo of a workspace; `local_path` is this machine's mapping. */
export interface RepoRef {
  name: string;
  remote: string;
  default_branch: string;
  local_path: string | null;
}

export interface Member {
  user: string;
  role: MemberRole;
}

/** `workspace.toml` (`parzi_core::workspace::Workspace`). */
export interface Workspace {
  name: string;
  kind: WorkspaceKind;
  repos: RepoRef[];
  members: Member[];
}

/** What the app knows about the GitHub credential — never the token itself. */
export interface GithubStatus {
  connected: boolean;
  login: string;
  source: "token" | "none";
  /** `gh` is on PATH, so "Use gh" is worth offering. */
  gh: boolean;
}

export interface GithubOrg {
  login: string;
  name: string;
}

export interface GithubRepo {
  name: string;
  full_name: string;
  default_branch: string;
  clone_url: string;
  private: boolean;
}

/** What one `workspace_sync` did (`parzi_core::workspace::sync::SyncReport`). */
export interface SyncReport {
  committed: boolean;
  pushed: boolean;
  pulled: boolean;
  /** Paths left for a human; non-empty means nothing was committed. */
  conflicts: string[];
}

/** Progress of one `repo_clone_or_map`, pushed on `parzi://repo-clone`. */
export interface CloneEvent {
  workspace: string;
  repo: string;
  phase: "cloning" | "done" | "error";
  line: string;
  path: string;
}

export const hub = {
  workspaces: () => invoke<string[]>("workspace_list"),
  workspace: (name: string) => invoke<Workspace>("workspace_get", { name }),
  createWorkspace: (name: string, kind: WorkspaceKind) =>
    invoke<Workspace>("workspace_create", { name, kind }),
  addRepos: (workspace: string, repos: RepoRef[]) =>
    invoke<Workspace>("workspace_add_repos", { workspace, repos }),
  /** Delete a hub workspace, its deck projects' role sessions and its chats. */
  deleteWorkspace: (workspace: string) =>
    invoke<number>("workspace_delete", { workspace }),
  /** Move a legacy `~/.parzi/projects/<name>` into a hub workspace of the same name. */
  migrateWorkspace: (name: string) =>
    invoke<Workspace>("workspace_migrate", { name }),
  /** Round 1's syncer is the workspace's own git repo (§1.1). */
  syncWorkspace: (workspace: string) =>
    invoke<SyncReport>("workspace_sync", { workspace }),
  /** Paste a token, or pass nothing to borrow the one `gh` holds. */
  githubConnect: (token?: string) =>
    invoke<GithubStatus>("github_connect", { token: token ?? null }),
  githubStatus: () => invoke<GithubStatus>("github_status"),
  orgs: () => invoke<GithubOrg[]>("github_list_orgs"),
  /** `org` empty = the signed-in user's own repos. */
  repos: (org: string) => invoke<GithubRepo[]>("github_list_repos", { org }),
  /**
   * Map an existing checkout (`local_path`) or clone in the background.
   * Resolves with the destination path; progress arrives on `parzi://repo-clone`.
   */
  cloneOrMap: (args: {
    workspace: string;
    repo: RepoRef;
    localPath?: string;
    destRoot?: string;
  }) =>
    invoke<string>("repo_clone_or_map", {
      workspace: args.workspace,
      repo: args.repo,
      localPath: args.localPath ?? null,
      destRoot: args.destRoot ?? null,
    }),
  createProject: (args: {
    workspace: string;
    title: string;
    repos: string[];
    roster: Roster;
    budgetUsd?: number | null;
  }) =>
    invoke<Project>("project_create", {
      workspace: args.workspace,
      title: args.title,
      repos: args.repos,
      roster: args.roster,
      budgetUsd: args.budgetUsd ?? null,
    }),
  /** Screenshot aid: `PARZI_UI_STATE=new-workspace:repos` opens that step. */
  uiState: () => invoke<string>("ui_state"),
};

export function onCloneEvent(cb: (e: CloneEvent) => void) {
  return listen<CloneEvent>("parzi://repo-clone", (ev) => cb(ev.payload));
}

/* ---------- deck (hub round 1 — PLAN.md §8) ---------- */

/**
 * The five `status:` values of PROJECT.md, spelled exactly as the wire
 * spells them: `parzi_core::project::Status` is
 * `#[serde(rename_all = "lowercase")]`, so a capitalised union never
 * matches. `tests/fixtures/project-status.json` holds that serde output and
 * `deckState.test.ts` asserts this list is it.
 */
export const PROJECT_STATUSES = ["drafting", "planned", "running", "done", "parked"] as const;

export type ProjectStatus = (typeof PROJECT_STATUSES)[number];

/** One acceptance criterion from PROJECT.md `## What`. */
export interface Criterion {
  text: string;
  done: boolean;
}

/** `provider/model` per role, as written in PROJECT.md `roster:`. */
export interface Roster {
  header: string;
  orchestrator: string;
  coder: string;
}

/** PROJECT.md, parsed (`parzi_core::project::Project`). */
export interface Project {
  slug: string;
  title: string;
  workspace: string;
  repos: string[];
  roster: Roster;
  budget_usd: number | null;
  status: ProjectStatus;
  /** Globs whose lease transfer needs a human. */
  critical: string[];
  why: string;
  what: Criterion[];
  constraints: string[];
}

/** One task line of PLAN.md (`parzi_core::plan::Task`). */
export interface Task {
  id: string;
  title: string;
  repo: string;
  scope: string[];
  after: string[];
  critical: boolean;
  done: boolean;
  acceptance: string[];
}

export interface LanePlan {
  name: string;
  tasks: Task[];
}

export interface Sprint {
  title: string;
  target: string;
  lanes: LanePlan[];
}

/** PLAN.md v1, parsed (`parzi_core::plan::Plan`). */
export interface Plan {
  sprints: Sprint[];
}

/**
 * The journal vocabulary, spelled as the wire spells it: the same mistake
 * as `ProjectStatus` above, but `parzi_core::journal::Kind` is
 * `#[serde(rename_all = "snake_case")]`, so `PlanChanged` is `plan_changed`.
 * A capitalised union made every task in the Plan tab read as pending.
 */
export const JOURNAL_KINDS = [
  "claim", "release", "request", "grant", "deny", "convene",
  "handoff", "block", "plan_changed", "approve", "note",
] as const;

export type JournalKind = (typeof JOURNAL_KINDS)[number];

/** One line of `<project>/journal.jsonl`. */
export interface JournalLine {
  at: string;
  who: string;
  kind: JournalKind;
  task: string | null;
  text: string;
}

/** A rough plan the header wrote to `<project>/drafts/<n>.md`. */
export interface Draft {
  name: string;
  title: string;
  content: string;
  path: string;
}

/** What `project_audit` hands back: the orchestrator's <= 15-line summary. */
export interface AuditResult {
  summary: string;
  plan: Plan | null;
}

/** What `project_open` ensures: the project and its role sessions. */
export interface ProjectOpen {
  project: Project;
  header_session: string;
  orchestrator_session: string | null;
}

/** Text plus the mtime etag the poll uses to skip unchanged renders. */
export interface TextDoc {
  text: string;
  etag: string;
}

/** Commands may answer a bare value or `{value, etag}`; normalise both. */
function unwrap<T>(v: T, key: string): T {
  const o = v as unknown as Record<string, unknown>;
  return o && typeof o === "object" && key in o ? (o[key] as T) : v;
}

function asTextDoc(v: string | TextDoc): TextDoc {
  return typeof v === "string" ? { text: v, etag: String(v.length) } : v;
}

export const deck = {
  open: (workspace: string, slug: string) =>
    invoke<ProjectOpen>("project_open", { workspace, slug }),
  get: (workspace: string, slug: string) =>
    invoke<Project>("project_get", { workspace, slug }),
  save: (project: Project) => invoke<Project>("project_save", { project }),
  list: (workspace: string) =>
    invoke<Project[]>("project_list", { workspace }),
  /** Rename a project's title (the slug never moves). */
  rename: (workspace: string, slug: string, title: string) =>
    invoke<Project>("project_rename", { workspace, slug, title }),
  /** Delete a deck project and its role sessions. */
  remove: (workspace: string, slug: string) =>
    invoke<number>("project_delete", { workspace, slug }),
  drafts: (workspace: string, slug: string) =>
    invoke<Draft[]>("project_drafts", { workspace, slug }),
  /** Send one draft to the orchestrator: writes PLAN.md, returns the summary. */
  audit: async (workspace: string, slug: string, draft: string) => {
    const r = await invoke<AuditResult | string>("project_audit", { workspace, slug, draft });
    return typeof r === "string" ? { summary: r, plan: null } : r;
  },
  /** Human approval of the audited plan: status Planned, sprint 1 dispatched. */
  approve: (workspace: string, slug: string) =>
    invoke<Project>("project_approve", { workspace, slug }),
  /** Deterministic STATUS.md — no model, safe to poll. */
  status: async (workspace: string, slug: string) =>
    asTextDoc(await invoke<string | TextDoc>("project_status", { workspace, slug })),
  plan: async (workspace: string, slug: string) =>
    unwrap(await invoke<Plan>("project_plan", { workspace, slug }), "plan"),
  journal: async (workspace: string, slug: string) =>
    unwrap(await invoke<JournalLine[]>("project_journal", { workspace, slug }), "lines"),
  /**
   * Ask a role. The header answers "what are we building / what's happening";
   * the Direct toggle sends the same box to the orchestrator. Returns the
   * session id the reply streams into.
   */
  ask: async (
    open: ProjectOpen,
    to: "header" | "orchestrator",
    prompt: string,
    opts: { model?: string; effort?: string; attachments?: string[] } = {},
  ) => {
    const session = to === "header" ? open.header_session : open.orchestrator_session;
    return api.sendMessage({
      sessionId: session ?? undefined,
      project: open.project.slug,
      lane: to,
      model: opts.model || (to === "header" ? open.project.roster.header : open.project.roster.orchestrator),
      prompt,
      cwd: "",
      effort: opts.effort,
      attachments: opts.attachments,
    });
  },
};
