<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    api, hub, deck, onRunEvent,
    type SessionMeta, type ChatEvent, type ModelRow, type UiEvent,
    type ProjectView, type ProjectRoster, type Project, type InspectorArtifact, type InspectorDoc, type DocEntry, type SwarmNode
  } from "./lib/api";
  import RightPanel from "./lib/inspector/RightPanel.svelte";

  import Titlebar from "./lib/Titlebar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import Thread from "./lib/Thread.svelte";
  import Settings from "./lib/Settings.svelte";
  import SettingsNav from "./lib/SettingsNav.svelte";
  import NewWorkspace from "./lib/workspace/NewWorkspace.svelte";
  import NewProject from "./lib/workspace/NewProject.svelte";
  import { ensureModels, modelRows } from "./lib/modelStore";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce, changesThreadList } from "./lib/threadList";
  import { checkForUpdates, checkForUpdatesSoon } from "./lib/updateStore";
  import Icon from "./lib/Icon.svelte";

  const RM = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const smooth = RM ? { duration: 0 } : { duration: 200, easing: cubicOut };
  const smoothFast = RM ? { duration: 0 } : { duration: 140, easing: cubicOut };

  let bg = "";
  let threads: SessionMeta[] = [];
  let models: ModelRow[] = [];
  let projects: ProjectView[] = [];
  // Shared catalog: startup populates, Settings > Models reuses live.
  modelRows.subscribe((v) => {
    models = v;
  });

  let curProject = "default";
  let projectRoster: ProjectRoster | null = null;
  let projectPlan = "";
  let activeThreadId: string | null = null;
  let activeMeta: SessionMeta | null = null;
  let events: ChatEvent[] = [];
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTokens = 0;
  let liveCost = 0;
  let compacting = false;
  $: pickedWindow = (() => {
    const [provider, id] = model.includes("/") ? model.split(/\/(.*)/s) : ["", ""];
    const row = models.find((r) => r.provider === provider);
    return row?.models.find((m) => m.id === id)?.context_limit ?? 0;
  })();
  $: contextUsed = activeMeta?.context_tokens ?? 0;
  $: contextLimit = activeMeta?.context_limit || pickedWindow;

  let liveTools: { id: string; name: string; label: string; running: boolean; ok: boolean; ms: number }[] = [];
  let input = "";
  let model = "auto";
  let effort: "low" | "medium" | "high" | "extra" | "ultra" = "medium";
  let permission = "full";

  function pillMode(): string {
    if (permission === "supervised" || permission === "edits") return permission;
    return "auto";
  }
  let mode: "chat" | "plan" | "build" = "chat";
  let curLane = "";
  let attachments: string[] = [];

  let showSettings = false;
  let settingsSection = "general";

  function openSettings(section?: string, anchor?: string) {
    if (section) {
      settingsSection = section;
    }
    palette = false;
    showSettings = true;
    if (anchor) {
      void (async () => {
        for (let i = 0; i < 10; i++) {
          await tick();
          await new Promise((r) => setTimeout(r, 60));
          const el = document.getElementById(anchor);
          if (el) {
            el.scrollIntoView({ block: "start", behavior: RM ? "auto" : "smooth" });
            break;
          }
        }
      })();
    }
  }
  let sidebarOpen = true;

  type HubView =
    | { kind: "new-workspace"; step: string }
    | { kind: "new-project"; workspace: string; step: string };
  let hubView: HubView | null = null;
  let hubTick = 0;
  let wsNames: string[] = [];
  let wsRoots: Record<string, string> = {};
  let wsProjects: Project[] = [];

  async function loadWorkspaces() {
    try {
      wsNames = await hub.workspaces();
    } catch {
      wsNames = [];
    }
    const pairs = await Promise.all(
      wsNames.map(async (n) => {
        const w = await hub.workspace(n).catch(() => null);
        return [n, w?.repos.find((r) => r.local_path)?.local_path ?? ""] as const;
      }),
    );
    wsRoots = Object.fromEntries(pairs);
  }
  $: if (hubTick >= 0) void loadWorkspaces();
  $: curWorkspace = wsNames.includes(curProject) ? curProject : "";
  let sideFilter: string | null = null;
  $: legacyNames = projects
    .map((p) => p.name)
    .filter((n) => n && n !== "default" && !wsNames.includes(n));
  $: panelWs = sideFilter
    ? (wsNames.includes(sideFilter) ? sideFilter : "")
    : curWorkspace;
  $: if (hubTick >= 0) void loadWsProjects(panelWs);
  async function loadWsProjects(name: string) {
    wsProjects = name ? await deck.list(name).catch(() => []) : [];
  }

  let dockProject: { workspace: string; slug: string } | null = null;
  $: if (dockProject && dockProject.workspace !== panelWs) dockProject = null;

  /** Pick the workspace a draft chat will belong to; the typed prompt stays.
      A sent thread keeps its workspace, so switching with one open parks it
      and opens a fresh draft instead of moving it. Blocked mid-run: the live
      tail belongs to the old thread. */
  const EFFORT_IDS = ["low", "medium", "high", "extra", "ultra"] as const;

  /** Adopt a hub workspace's composer defaults, but only where the composer
      is still on app defaults — never clobber an explicit pick. */
  async function adoptWorkspaceDefaults(name: string) {
    if (!wsNames.includes(name)) return;
    let ws;
    try {
      ws = await hub.workspace(name);
    } catch {
      return;
    }
    const took: string[] = [];
    const wantModel = (ws.defaults?.model ?? "").trim();
    if (wantModel && model === "auto" && (wantModel === "auto" || wantModel.includes("/"))) {
      model = wantModel;
      took.push(wantModel);
    }
    const wantEffort = (ws.defaults?.effort ?? "").trim();
    if (
      wantEffort &&
      (EFFORT_IDS as readonly string[]).includes(wantEffort) &&
      effort === "medium"
    ) {
      effort = wantEffort as typeof effort;
      took.push(wantEffort);
    }
    if (took.length) toast(`${displayName(name)} defaults: ${took.join(" · ")}`);
  }

  async function selectWorkspace(name: string) {
    const target = name || "default";
    if (liveRun) {
      toast("Wait for the run to finish before switching workspaces", true);
      return;
    }
    if ((activeThreadId || events.length) && target !== curProject) {
      const keepInput = input;
      const keepAttach = attachments;
      newThread();
      input = keepInput;
      attachments = keepAttach;
      curProject = target;
      await refreshBranch();
      projectRoster = null;
      projectPlan = "";
      try {
        projectRoster = await api.getProjectRoster(target);
      } catch {}
      try {
        projectPlan = await api.getProjectPlan(target);
      } catch {}
      toast(`New draft in ${displayName(target)}`);
      await adoptWorkspaceDefaults(target);
      return;
    }
    curProject = target;
    await refreshBranch();
    await adoptWorkspaceDefaults(target);
  }

  async function handleMigrateProject(name: string) {
    try {
      await hub.migrateWorkspace(name);
      hubTick += 1;
      await loadProjects();
      await loadThreads();
      sideFilter = name;
      toast(`Moved ${name} to workspaces`);
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function handleDeleteLegacyProject(name: string) {
    if (sideFilter === name) sideFilter = null;
    await handleDeleteProject(name);
  }

  async function handleDeleteWorkspace(name: string) {
    if (sideFilter === name) sideFilter = null;
    try {
      await hub.deleteWorkspace(name);
      await handleWorkspaceDeleted(name);
    } catch (e) {
      toast(String(e), true);
    }
  }

  function handleNewThread() {
    if (sideFilter) void startDraftInProject(sideFilter);
    else newThread();
  }

  async function handleRenameDeckProject(workspace: string, slug: string, title: string) {
    try {
      await deck.rename(workspace, slug, title);
      hubTick += 1;
      toast("Project renamed");
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function handleDeleteDeckProject(workspace: string, slug: string) {
    try {
      await deck.remove(workspace, slug);
      hubTick += 1;
      if (dockProject && dockProject.workspace === workspace && dockProject.slug === slug) {
        dockProject = null;
      }
      await loadThreads();
      toast("Project deleted");
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function handleWorkspaceDeleted(name: string) {
    hubTick += 1;
    if (dockProject && dockProject.workspace === name) dockProject = null;
    if (curProject === name) {
      curProject = "default";
      if (activeThreadId || events.length) newThread();
      await refreshBranch();
    }
    await loadThreads();
    if (liveRun && !threads.some((t) => t.id === liveRun)) {
      liveRun = null;
      clearLive();
    }
    if (activeThreadId && !threads.some((t) => t.id === activeThreadId)) {
      activeThreadId = null;
      activeMeta = null;
      events = [];
    }
    toast(`Workspace ${name} deleted`);
  }

  function openNewWorkspaceWizard(step = "") {
    showSettings = false;
    hubView = { kind: "new-workspace", step };
  }

  function openNewProjectWizard(workspace: string, step = "") {
    showSettings = false;
    hubView = { kind: "new-project", workspace, step };
  }

  function openProject(workspace: string, slug: string) {
    dockProject = { workspace, slug };
    showSettings = false;
    openRightBar("project");
  }

  function closeHubView() {
    hubView = null;
  }

  async function applyUiState() {
    let state = "";
    try {
      state = (await hub.uiState()).trim();
    } catch {}
    if (!state && typeof location !== "undefined") {
      state = new URLSearchParams(location.search).get("ui_state")?.trim() ?? "";
    }
    if (!state) return;
    const [screen, step = ""] = state.split(":");
    if (screen === "new-workspace") openNewWorkspaceWizard(step);
    else if (screen === "new-project") openNewProjectWizard("", step);
    else if (screen === "chats") hubView = null;
    else if (screen === "thread" && step) openThread(step);
    else if (screen === "menu" && step) {
      setTimeout(() => document.querySelector<HTMLButtonElement>(`.${step}-zone button`)?.click(), 1500);
    }
  }

  const displayName = (n: string) => (n === "default" ? "Inbox" : n);

  async function startDraftInProject(name: string) {
    const target = name.trim() || "default";
    if (target !== curProject) {
      curProject = target;
      await refreshBranch();
      projectRoster = null;
      projectPlan = "";
      try {
        projectRoster = await api.getProjectRoster(target);
      } catch {}
      try {
        projectPlan = await api.getProjectPlan(target);
      } catch {}
      await adoptWorkspaceDefaults(target);
    }
    newThread();
  }
  let modelMenu = false;
  let modelQuery = "";

  let sending = false;
  let liveRun: string | null = null;
  let approval: { key: string; call: { id: string; name: string; args: unknown; lane: string } } | null = null;
  let toasts: { id: number; text: string; err: boolean }[] = [];
  let toastSeq = 0;
  let scrollEl: HTMLElement | null = null;

  function toast(text: string, err = false) {
    const id = ++toastSeq;
    toasts = [...toasts, { id, text, err }];
    setTimeout(() => {
      toasts = toasts.filter((t) => t.id !== id);
    }, 3800);
  }

  $: currentProjectView = projects.find((p) => p.name === curProject);
  $: currentRoot = currentProjectView?.root || wsRoots[curProject] || "";

  async function loadProjects() {
    try {
      projects = await api.listProjects();
    } catch {}
  }

  // E7: every `list_threads` re-reads every session's meta.json on the Rust
  // side, so the fifteen call sites go through one 100 ms coalescer instead
  // of firing per event. `loadThreads()` still resolves after a real fetch,
  // so `await loadThreads()` callers keep reading fresh `threads`.
  let listThreadCalls = 0;
  async function fetchThreads() {
    try {
      if (import.meta.env.DEV) console.debug(`[E7] list_threads #${++listThreadCalls}`);
      threads = await api.listThreads();
      // A purged/finished session can vanish under us: drop a stale selection
      // instead of highlighting nothing while the stage shows a dead thread.
      // Draft input is kept — only the dead selection is cleared.
      if (activeThreadId && !liveRun && !threads.some((t) => t.id === activeThreadId)) {
        activeThreadId = null;
        activeMeta = null;
        events = [];
      }
    } catch {}
  }
  const loadThreads = coalesce(fetchThreads, 100);

  async function refreshBranch() {
    if (currentRoot) {
      try {
        branch = await api.gitBranch(currentRoot);
      } catch {
        branch = "";
      }
    } else {
      branch = "";
    }
  }

  async function switchProject(name: string) {
    hubView = null;
    curProject = name;
    await refreshBranch();
    projectRoster = null;
    projectPlan = "";
    try {
      projectRoster = await api.getProjectRoster(name);
    } catch {}
    try {
      projectPlan = await api.getProjectPlan(name);
    } catch {}

    const wsThreads = threads
      .filter((t) => (t.project || "default") === name)
      .sort((a, b) => +new Date(b.updated) - +new Date(a.updated));
    if (wsThreads.length > 0) {
      await openThread(wsThreads[0].id);
    } else {
      newThread();
    }
  }

  async function openThread(id: string, keepLive = false) {
    hubView = null;
    activeThreadId = id;
    if (!keepLive) clearLive();
    liveTokens = 0;
    liveCost = 0;
    approval = null;
    try {
      const [meta, ev] = await api.getThread(id);
      activeMeta = meta;
      events = ev;
      curProject = meta.project || "default";
      curLane = meta.lane || "";
      if (meta.model && meta.model.includes("/")) {
        model = meta.model;
      }
    } catch (e) {
      toast(String(e), true);
    }
    if (keepLive) clearLive();
    await tick();
    if (scrollEl) {
      scrollEl.style.scrollBehavior = "auto";
      scrollEl.scrollTop = scrollEl.scrollHeight;
      scrollEl.style.scrollBehavior = "";
    }
  }

  function newThread() {
    hubView = null;
    activeThreadId = null;
    activeMeta = null;
    events = [];
    live = "";
    liveReasoning = "";
    liveTokens = 0;
    liveCost = 0;
    liveTools = [];
    approval = null;
    input = "";
    attachments = [];
  }

  function openPlanner() {
    mode = "plan";
    toast("Plan mode — the model plans, it does not touch the tree");
  }

  async function compactThread(focus = "") {
    if (!activeThreadId) return toast("Open a chat to compact it", true);
    if (liveRun === activeThreadId) return toast("Wait for the run to finish, then compact", true);
    if (compacting) return;
    const id = activeThreadId;
    compacting = true;
    try {
      await api.compactThread(id, focus);
      if (activeThreadId === id) await refreshEvents();
      toast("Conversation compacted");
    } catch (e) {
      toast(String(e), true);
    } finally {
      compacting = false;
    }
  }

  async function send() {
    const rawPrompt = input.trim();
    const compactCmd = /^\/compact(?:\s+([\s\S]*))?$/.exec(rawPrompt);
    if (compactCmd) {
      input = "";
      await compactThread(compactCmd[1]?.trim() ?? "");
      return;
    }
    if (!rawPrompt || sending || liveRun || !model) return;
    sending = true;
    input = "";
    const files = [...attachments];
    attachments = [];
    const cwd = currentRoot;
    let prompt = rawPrompt;
    if (mode === "plan") {
      prompt = "Plan only — do not edit files or run commands. Output the plan:\n\n" + rawPrompt;
    }

    const optimisticUser = { kind: "user", text: rawPrompt } as ChatEvent;
    events = [...events, optimisticUser];

    try {
      const sid = await api.sendMessage({
        sessionId: activeThreadId ?? undefined,
        project: curProject,
        lane: curLane,
        model,
        prompt,
        cwd,
        effort,
        attachments: files,
        mode: pillMode(),
      });
      activeThreadId = sid;
      liveRun = sid;
      live = "";
      liveReasoning = "";
      liveTools = [];
      await loadThreads();
      try {
        const [meta, ev] = await api.getThread(sid);
        activeMeta = meta;
        events = ev;
        curProject = meta.project || curProject;
        curLane = meta.lane || curLane;
      } catch {}
      await tick();
      if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
      const queued = threads.find((t) => t.id === sid)?.status === "queued";
      if (queued) {
        liveRun = null;
        toast("queued — starts when a slot frees (kill a run to jump in)");
      }
    } catch (e) {
      input = rawPrompt;
      attachments = files;
      events = events.filter((ev) => ev !== optimisticUser);
      toast(String(e), true);
    } finally {
      sending = false;
    }
  }

  async function fork(id: string) {
    try {
      const m = await api.forkThread(id);
      await loadThreads();
      await openThread(m.id);
      toast("forked thread");
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function newSubsession(parentId: string) {
    try {
      const m = await api.createSubsession({ parentId });
      await loadThreads();
      await openThread(m.id);
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function askHeader(prompt: string) {
    try {
      const headerModel = projectRoster?.header?.model?.trim() || model;
      const sid = await api.sendMessage({
        project: curProject,
        lane: curLane,
        model: headerModel,
        prompt: `You are the Header (Architect) agent for project ${curProject}. Answer strategically, reference the living PLAN.md, and propose concrete plan updates when asked:\n\n${prompt}`,
        cwd: currentRoot,
        effort,
        mode: pillMode(),
      });
      await loadThreads();
      await openThread(sid);
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function executePlan() {
    try {
      const m = projectPlan.match(/^-\s*\[\s\]\s*(.+)$/m);
      const task = m ? m[1].replace(/\[lane:[^\]]+\]/, "").trim() : "Work the living plan";
      const implModel = projectRoster?.implementation?.model?.trim() || model;
      const sid = await api.sendMessage({
        project: curProject,
        lane: curLane,
        model: implModel,
        prompt: `You are the Orchestrator for project ${curProject}. Living plan task: ${task}. Break it into lane steps, spawn implementation workers via lane.dispatch / session.spawn, and sync checkboxes back to PLAN.md via plan.update.`,
        cwd: currentRoot,
        effort,
        mode: pillMode(),
      });
      await loadThreads();
      await openThread(sid);
      toast("Orchestrator dispatched");
    } catch (e) {
      toast(String(e), true);
    }
  }

  $: parentTitle = (() => {
    const pid = activeMeta?.parent_id;
    if (!pid) return null;
    return threads.find((t) => t.id === pid)?.title ?? "parent thread";
  })();
  $: childSubs = activeThreadId
    ? threads
        .filter((t) => t.parent_id === activeThreadId)
        .sort((a, b) => +new Date(a.created) - +new Date(b.created))
        .map((t) => ({ id: t.id, title: t.title, status: t.status }))
    : [];

  async function handleBarCommand(name: string) {
    switch (name) {
      case "new":
        newThread();
        break;
      case "clear":
        newThread();
        break;
      case "compact":
        compactThread();
        break;
      case "plan":
        openPlanner();
        break;
      case "auto":
        model = "auto";
        toast("Smart Auto routing on");
        break;
      case "effort": {
        const order = ["low", "medium", "high", "extra", "ultra"] as const;
        // Legacy "med" (old defaults) still resolves.
        const cur = (effort as string) === "med" ? "medium" : effort;
        effort = order[(order.indexOf(cur) + 1) % order.length] ?? "medium";
        toast(`effort: ${effort}`);
        break;
      }
        break;
      case "fork":
        if (activeThreadId) fork(activeThreadId);
        else toast("nothing to fork", true);
        break;
      case "subsession":
        if (activeThreadId) newSubsession(activeThreadId);
        else toast("open a thread first", true);
        break;
      case "kill":
        if (liveRun) stopRun();
        else toast("nothing running", true);
        break;
      case "doctor":
        openSettings("system");
        break;
      case "keys":
        openSettings("models");
        break;
      case "help":
        toast("/plan /auto /model /effort /new /clear /compact /fork /subsession /kill /doctor");
        break;
    }
  }

  async function stopRun() {    if (liveRun) {
      try {
        await api.killRun(liveRun);
        toast("Agent stopped");
      } catch (e) {
        toast(String(e), true);
      }
    }
  }

  async function killSession(id: string) {
    try {
      await api.killRun(id);
      toast("Run stopped");
      await loadThreads();
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function handleDeleteThread(id: string) {
    try {
      const n = await api.deleteThread(id);
      if (liveRun === id) {
        liveRun = null;
        live = "";
        liveReasoning = "";
        liveTools = [];
      }
      if (activeThreadId === id) {
        activeThreadId = null;
        activeMeta = null;
        events = [];
      }
      await loadThreads();
      // Deleting a parent drops its open child too: clear a stale selection.
      if (activeThreadId && !threads.some((t) => t.id === activeThreadId)) {
        activeThreadId = null;
        activeMeta = null;
        events = [];
      }
      if (liveRun && !threads.some((t) => t.id === liveRun)) {
        liveRun = null;
        live = "";
        liveReasoning = "";
        liveTools = [];
      }
      toast(n > 1 ? `deleted ${n} sessions` : "thread deleted");
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function handleDeleteProject(name: string) {
    const target = name.trim();
    if (!target || target === "default") return;
    try {
      const n = await api.deleteProject(target);
      await loadProjects();
      await loadThreads();
      if (curProject === target) {
        await switchProject("default");
      }
      toast(n > 0 ? `workspace ${target} deleted (${n} session${n === 1 ? "" : "s"})` : `workspace ${target} deleted`);
    } catch (e) {
      toast(String(e), true);
    }
  }

  /* ---------- Right inspector deck (Project + Docs + Agents) ---------- */
  const RB_KEY = "parzi.rightbar";
  const rbSaved = (() => {
    try { return JSON.parse(localStorage.getItem(RB_KEY) ?? "{}") as { width?: number; auto?: boolean; tab?: string }; } catch { return {}; }
  })();
  // Closed on cold boot: the stage stays clean until something asks for it.
  let rightBarOpen = false;
  type RightTab = "project" | "docs";
  let rightBarTab: RightTab = rbSaved.tab === "docs" ? "docs" : "project";
  let rightBarWidth = Math.min(680, Math.max(340, Number(rbSaved.width) || 420));
  let autoReveal = rbSaved.auto ?? true;
  /** Full view: the inspector deck covers the body for reading. Anything the
      deck does that lands on the *stage* — opening a thread, starting the
      new-project wizard — drops out of it first, or you act on a screen you
      cannot see. */
  let rbFull = false;
  $: if (!rightBarOpen) rbFull = false;
  let selectedArtifact: InspectorArtifact | null = null;
  let selectedDoc: InspectorDoc | null = null;
  let projectDocs: DocEntry[] = [];
  let docLoading = false;
  let rbEl: HTMLElement | null = null;
  let sessionTools: Record<string, { name: string; since: number; running: boolean }> = {};

  $: try { localStorage.setItem(RB_KEY, JSON.stringify({ width: rightBarWidth, auto: autoReveal, tab: rightBarTab })); } catch {}

  function openRightBar(tab: RightTab) {
    rightBarTab = tab;
    rightBarOpen = true;
  }
  function toggleRightBar(tab?: RightTab) {
    if (tab && rightBarOpen && rightBarTab !== tab) {
      rightBarTab = tab;
      return;
    }
    if (tab) rightBarTab = tab;
    rightBarOpen = !rightBarOpen;
  }

  function normArtifact(payload: any, fb?: { id?: string; title?: string; kind?: string; version?: number }): InspectorArtifact | null {
    const d = payload ?? {};
    const content = String(d.content ?? "");
    if (!content.trim()) return null;
    return {
      id: String(d.id ?? fb?.id ?? "artifact"),
      title: String(d.title ?? fb?.title ?? d.id ?? "artifact"),
      kind: String(d.kind ?? fb?.kind ?? "text"),
      language: String(d.language ?? ""),
      content,
      version: Number(d.version ?? fb?.version ?? 1),
    };
  }

  $: threadArtifacts = (() => {
    const out: InspectorArtifact[] = [];
    for (const e of events) {
      if (e.kind === "artifact") {
        const a = normArtifact(e.payload, { id: e.id, title: e.title, kind: e.artifact_kind, version: e.version });
        if (a) out.push(a);
      }
    }
    return out;
  })();

  function showArtifact(a: InspectorArtifact) {
    selectedArtifact = a;
    selectedDoc = null;
    openRightBar("docs");
  }

  async function openProjectDoc(entry: DocEntry) {
    docLoading = true;
    try {
      const content = await api.readTextFile(entry.path);
      selectedDoc = { title: entry.label, content, path: entry.path };
      selectedArtifact = null;
      openRightBar("docs");
    } catch (e) {
      toast(String(e), true);
    } finally {
      docLoading = false;
    }
  }

  async function openTranscript() {
    if (!activeThreadId) return;
    docLoading = true;
    try {
      const [, , md] = await api.getThread(activeThreadId);
      selectedDoc = { title: "session.md", content: md || "_Transcript is empty._" };
      selectedArtifact = null;
      openRightBar("docs");
    } catch (e) {
      toast(String(e), true);
    } finally {
      docLoading = false;
    }
  }

  async function pickDocFile() {
    try {
      const picked = await api.pickTextFile(currentRoot || undefined);
      if (picked) {
        await openProjectDoc({ label: picked.split(/[\\/]/).pop() || picked, path: picked, source: "root" });
      }
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function loadProjectDocs() {
    try {
      projectDocs = await api.listProjectDocs(curProject, currentRoot);
    } catch {
      projectDocs = [];
    }
  }
  $: if (curProject || currentRoot) loadProjectDocs();

  $: swarmNodes = (() => {
    if (!activeThreadId) return [] as SwarmNode[];
    const byId = new Map(threads.map((t) => [t.id, t]));
    let root = byId.get(activeThreadId);
    if (!root) return [] as SwarmNode[];
    const guard = new Set<string>();
    while (root.parent_id && byId.has(root.parent_id) && !guard.has(root.id)) {
      guard.add(root.id);
      root = byId.get(root.parent_id)!;
    }
    const out: SwarmNode[] = [];
    const walk = (t: SessionMeta, depth: number) => {
      if (out.some((n) => n.id === t.id) || out.length > 120) return;
      out.push({
        id: t.id, title: t.title, lane: t.lane, model: t.model, status: t.status,
        tokens: t.tokens_in + t.tokens_out, cost: t.cost_usd,
        parentId: depth === 0 ? null : t.parent_id ?? null, depth,
        tool: sessionTools[t.id] ?? null,
      });
      threads
        .filter((c) => c.parent_id === t.id)
        .sort((a, b) => +new Date(a.created) - +new Date(b.created))
        .forEach((c) => walk(c, depth + 1));
    };
    walk(root, 0);
    return out;
  })();
  $: liveAgentCount = swarmNodes.filter((n) => n.status === "active" || n.status === "queued").length;

  async function refreshEvents() {
    if (!activeThreadId) return;
    try {
      const [meta, ev] = await api.getThread(activeThreadId);
      activeMeta = meta;
      events = ev;
    } catch {}
  }

  const listedRuns = new Set<string>();

  const LIVE_FLUSH_MS = 60;
  let liveBuf = "";
  let liveBase = "";
  let liveBufThread: string | null = null;
  let liveTimer = 0;
  function flushLive() {
    liveTimer = 0;
    if (!liveBuf) return;
    if (live !== liveBase || activeThreadId !== liveBufThread) {
      liveBuf = "";
      return;
    }
    liveBase += liveBuf;
    live = liveBase;
    liveBuf = "";
    if (scrollEl && scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight < 160) {
      tick().then(() => {
        if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
      });
    }
  }
  function pushLive(text: string, session: string) {
    if (!liveBuf) {
      liveBase = live;
      liveBufThread = session;
    }
    liveBuf += text;
    if (liveTimer) return;
    liveTimer = window.setTimeout(() => requestAnimationFrame(flushLive), LIVE_FLUSH_MS);
  }

  function clearLive() {
    liveBuf = "";
    liveBase = "";
    live = "";
    liveReasoning = "";
    liveTools = [];
  }

  function onEvent(e: UiEvent) {
    if (e.kind === "approval") {
      approval = { key: e.key, call: e.call };
      return;
    }
    if (e.kind === "subsession_created") {
      loadThreads();
      return;
    }
    if (e.kind === "tool_call") {
      sessionTools = { ...sessionTools, [e.session]: { name: e.name, since: Date.now(), running: true } };
      if (e.session === activeThreadId && !liveTools.some((t) => t.id === e.id)) {
        liveTools = [...liveTools, { id: e.id, name: e.name, label: e.label || e.name, running: true, ok: true, ms: 0 }];
      }
    } else if (e.kind === "tool_result") {
      const cur = sessionTools[e.session];
      sessionTools = { ...sessionTools, [e.session]: { name: e.name, since: cur?.since ?? Date.now(), running: false } };
      if (e.session === activeThreadId) {
        let joined = false;
        liveTools = liveTools.map((t) => {
          if (!joined && t.running && (t.id === e.id || t.name === e.name)) {
            joined = true;
            return { ...t, running: false, ok: e.ok, ms: e.ms };
          }
          return t;
        });
      }
      if (e.name === "ui.show_artifact" && e.ok && e.session === activeThreadId) {
        refreshEvents().then(() => {
          const latest = threadArtifacts[threadArtifacts.length - 1];
          if (latest && autoReveal) showArtifact(latest);
        });
      }
    } else if (e.kind === "done" || e.kind === "error") {
      const { [e.session]: _gone, ...rest } = sessionTools;
      sessionTools = rest;
      if (liveRun === e.session) {
        liveRun = null;
      }
    }
    if (e.session !== activeThreadId) {
      const first = !listedRuns.has(e.session);
      if (e.kind === "done" || e.kind === "error") listedRuns.delete(e.session);
      else listedRuns.add(e.session);
      if (changesThreadList(e.kind, first)) loadThreads();
      return;
    }
    if (e.kind === "text") pushLive(e.text, e.session);
    else if (e.kind === "reasoning") liveReasoning += e.text;
    else if (e.kind === "notice") {
      toast(e.text);
      loadThreads();
    }
    else if (e.kind === "usage") {
      liveTokens += e.tokens_in + e.tokens_out;
      liveCost += e.cost_usd;
    } else if (e.kind === "context") {
      if (activeMeta) activeMeta = { ...activeMeta, context_tokens: e.used, context_limit: e.limit };
    } else if (e.kind === "done" || e.kind === "error") {
      if (e.kind === "error") toast(e.error, true);
      liveRun = null;
      if (activeThreadId) openThread(activeThreadId, true);
      else clearLive();
      loadThreads();
    } else if (e.kind !== "tool_call" && e.kind !== "tool_result") {
      loadThreads();
    }
  }

  $: allModelOptions = (() => {
    const list: { provider: string; id: string; label: string; auth: string }[] = [];
    for (const r of models) {
      for (const m of r.models) {
        list.push({
          provider: r.provider,
          id: `${r.provider}/${m.id}`,
          label: m.name || m.id,
          auth: r.auth,
        });
      }
    }
    return list;
  })();

  $: filteredModelOptions = allModelOptions.filter(
    (m) =>
      !modelQuery.trim() ||
      m.label.toLowerCase().includes(modelQuery.toLowerCase()) ||
      m.provider.toLowerCase().includes(modelQuery.toLowerCase())
  );

  // Command palette (Ctrl+K): actions + threads + models. Semantic buttons,
  // arrow/enter nav, Esc unwinds. Restores the pre-rewrite global shortcut.
  let palette = false;
  let palQuery = "";
  let palIndex = 0;

  interface PalItem {
    section: string;
    label: string;
    sub: string;
    run: () => void;
  }

  async function copyTranscript() {
    if (!activeThreadId) return;
    try {
      const [, , md] = await api.getThread(activeThreadId);
      await navigator.clipboard.writeText(md);
      toast("transcript copied");
    } catch (e) {
      toast(String(e), true);
    }
  }

  $: palActions = [
    { section: "Actions", label: "New chat", sub: "start a new conversation", run: () => { palette = false; newThread(); } },
    { section: "Actions", label: "New workspace", sub: "hub wizard", run: () => { palette = false; openNewWorkspaceWizard(); } },
    { section: "Actions", label: "Plan mode", sub: "no edits", run: () => { palette = false; openPlanner(); } },
    { section: "Actions", label: "Settings", sub: "models · connectors · skills", run: () => openSettings() },
    { section: "Actions", label: "Report issue", sub: "github", run: () => openSettings("system", "report-issue") },
    { section: "Actions", label: "Check for updates", sub: "stable channel", run: () => { palette = false; openSettings("system", "app-updates"); void checkForUpdates(true); } },
    ...(liveRun
      ? [{ section: "Actions", label: "Kill active run", sub: "stop now", run: () => { palette = false; stopRun(); } }]
      : []),
    ...(activeThreadId
      ? [
          { section: "Actions", label: "Fork this thread", sub: "branch", run: () => { palette = false; fork(activeThreadId as string); } },
          { section: "Actions", label: "Copy transcript", sub: "markdown", run: () => { palette = false; copyTranscript(); } },
        ]
      : []),
  ] as PalItem[];
  $: palThreads = threads
    .filter((t) => !palQuery || t.title.toLowerCase().includes(palQuery.toLowerCase()))
    .slice(0, 8)
    .map(
      (t) =>
        ({
          section: "Threads",
          label: t.title || "untitled",
          sub: t.model,
          run: () => {
            palette = false;
            openThread(t.id);
          },
        }) as PalItem
    );
  $: palModels = allModelOptions
    .filter(
      (o) =>
        !palQuery ||
        o.label.toLowerCase().includes(palQuery.toLowerCase()) ||
        o.id.toLowerCase().includes(palQuery.toLowerCase())
    )
    .slice(0, 6)
    .map(
      (o) =>
        ({
          section: "Models",
          label: o.label,
          sub: o.id,
          run: () => {
            palette = false;
            if (o.auth !== "ok") {
              toast(`${o.provider} needs a key — opening settings`, true);
              openSettings("models");
              return;
            }
            model = o.id;
          },
        }) as PalItem
    );
  $: palAll = [...palActions, ...palThreads, ...palModels];
  $: if (palIndex >= palAll.length && palAll.length) palIndex = 0;

  function onGlobalKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      palette = !palette;
      palQuery = "";
      palIndex = 0;
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
      e.preventDefault();
      newThread();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") {
      e.preventDefault();
      sidebarOpen = !sidebarOpen;
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === ",") {
      e.preventDefault();
      showSettings = !showSettings;
      return;
    }
    // Inspector deck: Ctrl+\ toggles, Ctrl+Shift+D too (some keyboards bury backslash).
    if ((e.ctrlKey || e.metaKey) && (e.key === "\\" || (e.shiftKey && e.key.toLowerCase() === "d"))) {
      e.preventDefault();
      toggleRightBar();
      return;
    }
    // Full deck view for reading projects and artifacts.
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      if (rightBarOpen) rbFull = !rbFull;
      return;
    }
    if (e.key === "F11") {
      e.preventDefault();
      import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
        const w = getCurrentWindow();
        w.isFullscreen().then((f) => w.setFullscreen(!f));
      });
      return;
    }
    if (palette && e.key === "ArrowDown") {
      e.preventDefault();
      palIndex = (palIndex + 1) % Math.max(1, palAll.length);
    } else if (palette && e.key === "ArrowUp") {
      e.preventDefault();
      palIndex = (palIndex - 1 + Math.max(1, palAll.length)) % Math.max(1, palAll.length);
    } else if (palette && e.key === "Enter") {
      e.preventDefault();
      palAll[palIndex]?.run();
    } else if (e.key === "Escape") {
      if (palette) palette = false;
      else if (hubView) closeHubView();
      else if (showSettings) showSettings = false;
      else if (rbFull) rbFull = false;
      else if (rightBarOpen && rbEl && rbEl.contains(document.activeElement)) rightBarOpen = false;
      else if (liveRun) stopRun();
    }
  }

  onMount(async () => {
    try {
      if (localStorage.getItem("parzi.scanlines") === "1") {
        document.documentElement.classList.add("scanlines");
      }
    } catch {}
    document.addEventListener("parzi:bg", (e) => {
      bg = (e as CustomEvent<string>).detail;
    });

    // Transparent frameless window: keep the 10px corner radius only while
    // floating — a maximized window must go square to meet the screen edges.
    import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => {
        const w = getCurrentWindow();
        const sync = () =>
          w
            .isMaximized()
            .then((m) => document.documentElement.classList.toggle("parzi-maximized", m))
            .catch(() => {});
        void sync();
        void w.onResized(() => void sync());
      })
      .catch(() => {});

    try {
      applyThemeCss(await api.getThemeCss());

      bg = await api.backgroundUrl();
      await ensureModels(false);
      await loadProjects();
      await loadThreads();
      await onRunEvent(onEvent);
      await applyUiState();

      // Smart Auto is the default; it routes through whichever
      // subscriptions are signed in. Nothing to pick at boot.
      // Update badge fills in quietly a few seconds later (never blocks paint).
      checkForUpdatesSoon();
    } catch (e) {
      toast(`Startup warning: ${e}`, true);
    }
  });
</script>

<div class="parzi-app-shell">
  <!-- Dynamic Background Wallpaper & Moody Atmosphere Grade -->
  <div class="background-backdrop">
    {#if bg}<img src={bg} alt="" class="bg-img" />{/if}
    <div class="bg-overlay" />
  </div>

  <div class="app-body">
    <div class="sb-wrap" class:closed={!sidebarOpen}>
    {#if showSettings}
    <div class="sb-pane" in:fly={{ x: -12, ...smooth }} out:fly={{ x: -8, ...smoothFast }}>
    <SettingsNav
      activeSection={settingsSection}
      authedCount={models.filter((m) => m.auth === "ok").length}
      on:select={(e) => { settingsSection = e.detail.id; }}
      on:back={() => (showSettings = false)}
    />
    </div>
    {:else}
    <div class="sb-pane" in:fly={{ x: -12, ...smooth }} out:fly={{ x: -8, ...smoothFast }}>
    <Sidebar
      {threads}
      {activeThreadId}
      {hubTick}
      sideFilter={sideFilter}
      legacyProjects={legacyNames}
      on:selectThread={(e) => openThread(e.detail.id)}
      on:newSubsession={(e) => newSubsession(e.detail.id)}
      on:newThread={handleNewThread}
      on:filterWorkspace={(e) => (sideFilter = e.detail.name)}
      on:migrateProject={(e) => handleMigrateProject(e.detail.name)}
      on:deleteLegacyProject={(e) => handleDeleteLegacyProject(e.detail.name)}
      on:deleteWorkspace={(e) => handleDeleteWorkspace(e.detail.name)}
      on:forkThread={(e) => fork(e.detail.id)}
      on:deleteThread={(e) => handleDeleteThread(e.detail.id)}
      on:killRun={(e) => killSession(e.detail.id)}
      on:openSettings={() => openSettings()}
      on:reportIssue={() => openSettings("system", "report-issue")}
      on:openUpdates={() => { openSettings("system", "app-updates"); void checkForUpdates(true); }}
      on:openPalette={() => { palette = true; }}
      on:toggleSidebar={() => (sidebarOpen = false)}
    />
    </div>
    {/if}
    </div>
    <div class="stage-col">
    {#if !rbFull}
    <Titlebar
      title={showSettings ? "Settings" : hubView?.kind === "new-workspace" ? "New workspace" : hubView?.kind === "new-project" ? "New project" : curWorkspace || "Inbox"}
      subtitle={showSettings ? settingsSection : hubView ? "" : activeMeta ? activeMeta.title : !activeThreadId ? "new draft" : ""}
      showExpand={!sidebarOpen}
      panelOpen={rightBarOpen}
      agentLive={liveAgentCount}
      on:expand={() => (sidebarOpen = true)}
      on:togglePanel={() => toggleRightBar()}
    />
    {/if}
    <main class="stage-container" class:settings-mode={showSettings}>
      {#if showSettings}
        <div class="stage-scroll settings-stage" in:fly={{ y: 12, ...smooth }} out:fly={{ y: 8, ...smoothFast }}>
          <Settings settingsTab={settingsSection} bareSection={settingsSection} currentProject={curProject} />
        </div>
      {:else if hubView?.kind === "new-workspace"}
        <div class="stage-scroll">
          <NewWorkspace
            initialStep={hubView.step}
            on:cancel={closeHubView}
            on:created={(e) => { hubTick += 1; hubView = null; void selectWorkspace(e.detail.name); }}
          />
        </div>
      {:else if hubView?.kind === "new-project"}
        <div class="stage-scroll">
          <NewProject
            workspace={hubView.workspace}
            initialStep={hubView.step}
            on:cancel={closeHubView}
            on:created={(e) => { hubTick += 1; openProject(e.detail.workspace, e.detail.slug); }}
          />
        </div>
      {:else}
        {#if activeThreadId}
          <div class="stage-scroll" bind:this={scrollEl}>
            <Thread {events} liveText={live} liveReasoning={liveReasoning} {liveTools} {approval} streaming={!!liveRun}
              {parentTitle} subsessions={childSubs} projectRoot={currentRoot}
              on:goParent={() => { if (activeMeta?.parent_id) openThread(activeMeta.parent_id); }}
              on:openSubsession={(e) => openThread(e.detail.id)}
              on:openArtifact={(e) => showArtifact(e.detail.artifact)}
 />
          </div>
        {:else}
          <div class="home-hero-stage"></div>
        {/if}
        <div class="omnibar-slot" class:hero={!activeThreadId} class:dock={!!activeThreadId}>
          <Omnibar
            bind:input
            bind:model
            bind:effort
            bind:mode
            bind:permission
            bind:attachments
            streaming={!!liveRun}
            currentProject={curProject}
            currentTask={null}
            currentSubfolder={null}
            projectRoot={currentRoot}
            {branch}
            tokens={liveTokens}
            {contextUsed}
            {contextLimit}
            {compacting}
            {models}
            on:send={send}
            on:stop={stopRun}
            on:modelChange={(e) => (model = e.detail.model)}
            on:command={(e) => handleBarCommand(e.detail.name)}
            on:openPlanner={openPlanner}
            workspaces={wsNames}
            workspace={curWorkspace}
            workspaceFixed={!!activeThreadId}
            on:workspaceChange={(e) => selectWorkspace(e.detail.workspace)}
            on:workspaceDeleted={(e) => handleWorkspaceDeleted(e.detail.workspace)}
            on:error={(e) => toast(e.detail.text, true)}
            on:newWorkspace={() => openNewWorkspaceWizard()}
          />
        </div>
      {/if}
    </main>
    </div>
    <!-- Right inspector deck: docked split, collapses to zero width -->
    <div class="rb-wrap" class:closed={!rightBarOpen} class:full={rbFull} style="--rb-w: {rightBarWidth}px" bind:this={rbEl}>
      <RightPanel
        tab={rightBarTab}
        width={rightBarWidth}
        {autoReveal}
        full={rbFull}
        artifact={selectedArtifact}
        artifacts={threadArtifacts}
        doc={selectedDoc}
        docs={projectDocs}
        {docLoading}
        {activeThreadId}
        workspace={panelWs}
        projects={wsProjects}
        selected={dockProject}
        on:close={() => (rightBarOpen = false)}
        on:toggleFull={() => (rbFull = !rbFull)}
        on:tab={(e) => (rightBarTab = e.detail.tab)}
        on:resize={(e) => (rightBarWidth = e.detail.width)}
        on:autoReveal={(e) => (autoReveal = e.detail.on)}
        on:openDoc={(e) => openProjectDoc(e.detail.entry)}
        on:openTranscript={openTranscript}
        on:openArtifact={(e) => showArtifact(e.detail.artifact)}
        on:pickFile={pickDocFile}
        on:openProject={(e) => openProject(e.detail.workspace, e.detail.slug)}
        on:newProject={(e) => { rbFull = false; openNewProjectWizard(e.detail.workspace); }}
        on:closeProject={() => (dockProject = null)}
        on:openSession={(e) => { rbFull = false; openThread(e.detail.id); }}
        on:openDraft={(e) => { selectedDoc = e.detail.doc; selectedArtifact = null; openRightBar("docs"); }}
        on:error={(e) => toast(e.detail.text, true)}
      />
    </div>
  </div>

  <!-- Toast Notification Stack -->
  <div class="toast-stack">
    {#each toasts as t (t.id)}
      <div class="toast-item" class:error-toast={t.err} transition:fade|local={{ duration: 140 }}>
        {t.text}
      </div>
    {/each}
  </div>


  {#if palette}
    <div class="palette-wrap" transition:fade={{ duration: 120 }}>
      <div class="palette" on:click|stopPropagation>
        <div class="menu-search">
          <input placeholder="Type a command, thread, or model…" bind:value={palQuery}
            on:input={() => (palIndex = 0)} />
        </div>
        <div class="menu-list">
          {#each palActions as a, i}
            {#if i === 0}<div class="menu-provider">Actions</div>{/if}
            <button class="mrow" class:on={i === palIndex} on:click={() => a.run()} on:mousemove={() => (palIndex = i)}>
              <span class="m-name">{a.label}</span><span class="act-hint">{a.sub}</span>
            </button>
          {/each}
          {#if palThreads.length}
            <div class="menu-provider">Threads</div>
            {#each palThreads as t, i}
              <button class="mrow" class:on={palActions.length + i === palIndex}
                on:click={() => t.run()} on:mousemove={() => (palIndex = palActions.length + i)}>
                <span class="m-name">{t.label}</span><span class="act-hint">{t.sub}</span>
              </button>
            {/each}
          {/if}
          {#if palModels.length}
            <div class="menu-provider">Models</div>
            {#each palModels as m, i}
              <button class="mrow" class:on={palActions.length + palThreads.length + i === palIndex}
                on:click={() => m.run()} on:mousemove={() => (palIndex = palActions.length + palThreads.length + i)}>
                <span class="m-name">{m.label}</span><span class="act-hint">{m.sub}</span>
              </button>
            {/each}
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<svelte:window on:keydown={onGlobalKey} />

<style>
  :global(html) {
    background: transparent;
  }
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent;
    color: var(--text);
    font-family: var(--parzi-font), Inter, system-ui, sans-serif;
    overflow: hidden;
  }
  :global(::selection) {
    background: var(--accent-mid);
    color: var(--text);
  }
  .parzi-app-shell {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    position: relative;
    overflow: hidden;
    border-radius: 10px;
  }
  :global(html.parzi-maximized) .parzi-app-shell {
    border-radius: 0;
  }
  .background-backdrop {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 0;
    overflow: hidden;
    background: var(--stage);
  }
  .bg-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.92;
  }
  .bg-overlay {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(ellipse at 50% 10%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 60%),
      radial-gradient(ellipse at 55% 42%, transparent 30%, rgba(0, 0, 0, var(--parzi-vignette)) 100%),
      color-mix(in srgb, var(--stage) var(--parzi-bg-dim-pct), transparent);
  }
  .app-body {
    flex: 1;
    display: flex;
    position: relative;
    z-index: 1;
    overflow: hidden;
  }
  .stage-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .sb-wrap {
    flex: none; width: 248px; margin-left: 0; opacity: 1; position: relative;
    transition: margin-left 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease;
  }
  .sb-wrap.closed { margin-left: -248px; opacity: 0; pointer-events: none; }
  .sb-wrap::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    right: -24px;
    width: 24px;
    background: linear-gradient(90deg, color-mix(in srgb, var(--parzi-sidebar) 28%, transparent), transparent);
    pointer-events: none;
    z-index: 3;
    opacity: 1;
    transition: opacity 240ms ease;
  }
  .sb-wrap.closed::after { opacity: 0; }
  .sb-pane { position: absolute; inset: 0; width: 248px; display: flex; flex-direction: column; min-height: 0; will-change: transform, opacity; }
  .rb-wrap {
    flex: none; width: var(--rb-w, 420px); margin-right: 0; opacity: 1; position: relative;
    min-height: 0; display: flex;
    transition: margin-right 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 0s;
  }
  .rb-wrap.closed {
    margin-right: calc(-1 * var(--rb-w, 420px)); opacity: 0; pointer-events: none; visibility: hidden;
    transition: margin-right 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 260ms;
  }
  @media (max-width: 1100px) {
    .rb-wrap {
      position: absolute; top: 38px; right: 0; bottom: 0; z-index: 20;
      width: min(var(--rb-w, 420px), calc(100% - 56px));
      background: var(--parzi-sidebar);
      box-shadow: var(--menu-shadow);
      transition: transform 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 0s;
    }
    .rb-wrap.closed {
      margin-right: 0; transform: translateX(100%);
      transition: transform 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 260ms;
    }
  }
  .rb-wrap.full {
    position: absolute; top: 0; left: 0; right: 0; bottom: 0; z-index: 30;
    width: auto; margin-right: 0; opacity: 1; visibility: visible;
    background: var(--parzi-sidebar);
    border-left: none;
    box-shadow: none;
  }
  .stage-container {
    flex: 1;
    display: flex;
    flex-direction: column;
    position: relative;
    overflow: hidden;
  }
  .stage-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .settings-stage {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    padding: 8px 32px 48px;
  }
  .omnibar-slot {
    z-index: 6; display: flex; justify-content: center; width: 100%;
    pointer-events: none;
    transition:
      top 520ms var(--ease-spring),
      bottom 520ms var(--ease-spring),
      transform 520ms var(--ease-spring),
      width 520ms var(--ease-spring),
      padding 520ms var(--ease-spring);
  }
  .omnibar-slot :global(.ob) {
    pointer-events: auto;
  }
  .omnibar-slot.hero {
    position: absolute; left: 50%; top: 46%;
    transform: translate(-50%, -50%);
    width: min(720px, 90%);
    padding: 0;
  }
  .omnibar-slot.dock {
    position: absolute; left: 50%; bottom: 12px; top: auto;
    transform: translate(-50%, 0);
    width: min(720px, calc(100% - 48px));
    padding: 0 0 8px;
  }
  @media (prefers-reduced-motion: reduce) {
    .omnibar-slot { transition: none; }
  }
  .home-hero-stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 32px;
    padding: 40px;
    max-width: 780px;
    margin: 0 auto;
    width: 100%;
    box-sizing: border-box;
  }
  .toast-stack {
    position: fixed;
    bottom: 16px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 300;
    pointer-events: none;
  }
  .toast-item {
    background: var(--menu);
    border: 1px solid var(--line-3);
    color: var(--text);
    font-size: 12px;
    padding: 8px 14px;
    border-radius: var(--radius-2);
    box-shadow: var(--menu-shadow);
  }
  .toast-item.error-toast {
    border-color: var(--bad-line);
    color: var(--bad);
  }
</style>
