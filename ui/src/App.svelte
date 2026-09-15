<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    api, onRunEvent,
    type SessionMeta, type ChatEvent, type ModelRow, type UiEvent,
    type ProjectView, type ProjectRoster, type InspectorArtifact, type InspectorDoc, type DocEntry, type SwarmNode
  } from "./lib/api";
  import RightPanel from "./lib/inspector/RightPanel.svelte";

  import Titlebar from "./lib/Titlebar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import ProjectMainPage from "./lib/ProjectMainPage.svelte";
  import Thread from "./lib/Thread.svelte";
  import Settings from "./lib/Settings.svelte";
  import SettingsNav from "./lib/SettingsNav.svelte";
  import { ensureModels, modelRows } from "./lib/modelStore";
  import { applyThemeCss } from "./lib/theme";
  import { checkForUpdates, checkForUpdatesSoon } from "./lib/updateStore";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
  /** True after an explicit project pick: stage shows the project Mission Control. */
  let projectSelected = false;
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
  let input = "";
  let model = "auto";
  let effort: "low" | "medium" | "high" | "extra" | "ultra" = "medium";
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
    // Optional in-tab jump (e.g. straight to the issue reporter): the
    // section mounts async, so retry until the anchor exists or we give up.
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
  // New-workspace popout (centered over the stage, not crammed in the sidebar).
  let showNewWs = false;
  let wsName = "";
  let wsRoot = "";
  let pickingWsFolder = false;
  $: wsFolderLabel = (() => {
    const p = wsRoot.trim();
    if (!p) return "";
    return p.split(/[\\/]/).filter(Boolean).pop() || p;
  })();

  function openNewWs() {
    wsName = "";
    wsRoot = "";
    showNewWs = true;
  }

  function closeNewWs() {
    showNewWs = false;
    wsName = "";
    wsRoot = "";
  }

  async function browseWsFolder() {
    if (pickingWsFolder) return;
    pickingWsFolder = true;
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Choose workspace folder",
        defaultPath: wsRoot.trim() || undefined,
      });
      if (typeof picked === "string" && picked) wsRoot = picked;
    } catch {
      // Native picker unavailable — workspace is still created without a root.
    } finally {
      pickingWsFolder = false;
    }
  }

  async function submitNewWs() {
    const name = wsName.trim();
    if (!name) return;
    closeNewWs();
    await handleCreateProject(name, wsRoot.trim());
  }

  function focusWsName(node: HTMLInputElement) {
    node.focus();
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
  $: currentRoot = currentProjectView?.root ?? "";

  async function loadProjects() {
    try {
      projects = await api.listProjects();
    } catch {}
  }

  async function loadThreads() {
    try {
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
    curProject = name;
    activeThreadId = null;
    activeMeta = null;
    events = [];
    projectSelected = true;
    await refreshBranch();
    // Mission Control data: roster + living plan load quietly, never block paint.
    projectRoster = null;
    projectPlan = "";
    try {
      projectRoster = await api.getProjectRoster(name);
    } catch {}
    try {
      projectPlan = await api.getProjectPlan(name);
    } catch {}
  }

  async function openThread(id: string) {
    activeThreadId = id;
    live = "";
    liveReasoning = "";
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
    await tick();
    if (scrollEl) {
      // Thread switches jump: bypass `scroll-behavior: smooth` so opening
      // a thread lands instantly instead of animating across the history.
      scrollEl.style.scrollBehavior = "auto";
      scrollEl.scrollTop = scrollEl.scrollHeight;
      scrollEl.style.scrollBehavior = "";
    }
  }

  function newThread() {
    activeThreadId = null;
    projectSelected = false;
    activeMeta = null;
    events = [];
    live = "";
    liveReasoning = "";
    liveTokens = 0;
    liveCost = 0;
    approval = null;
    input = "";
    attachments = [];
  }

  async function newThreadInProject(name: string) {
    if (name !== curProject) await switchProject(name);
    newThread();
    toast(`New conversation in ${name}`);
  }

  async function handleCreateProject(name: string, root: string) {
    name = name.trim();
    if (!name) return;
    try {
      await api.createProject(name, root.trim());
      await loadProjects();
      await switchProject(name);
      toast(`Workspace ${name} created`);
    } catch (e) {
      toast(String(e), true);
    }
  }

  function openPlanner() {
    mode = "plan";
    toast("Plan mode — the model plans, it does not touch the tree");
  }

  async function send() {
    const rawPrompt = input.trim();
    if (!rawPrompt || sending || liveRun || !model) return;
    sending = true;
    input = "";
    const files = [...attachments];
    attachments = [];
    const cwd = currentRoot;
    // Plan mode is a real mode: the model plans, it does not touch the tree.
    let prompt = rawPrompt;
    if (mode === "plan") {
      prompt = "Plan only — do not edit files or run commands. Output the plan:\n\n" + rawPrompt;
    }

    // Optimistic user bubble: the backend only returns the persisted
    // transcript on `done`, so without this the user stares at an empty
    // thread + "writing…" until the run finishes.
    const optimisticUser = { kind: "user", text: rawPrompt } as ChatEvent;
    // If this is a fresh thread there are no events yet; otherwise append.
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
      });
      activeThreadId = sid;
      liveRun = sid;
      await loadThreads();
      // Reconcile with the persisted transcript (replaces the optimistic
      // row with the real one; never wipes live assistant text).
      try {
        const [meta, ev] = await api.getThread(sid);
        activeMeta = meta;
        events = ev;
        curProject = meta.project || curProject;
        curLane = meta.lane || curLane;
      } catch {
        // Backend already accepted the prompt; keep the optimistic bubble.
      }
      await tick();
      if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
      const queued = threads.find((t) => t.id === sid)?.status === "queued";
      if (queued) {
        liveRun = null;
        toast("queued — starts when a slot frees (kill a run to jump in)");
      }
    } catch (e) {
      // Restore the prompt — losing a composed message on a send error
      // is the fastest way to lose trust.
      input = rawPrompt;
      attachments = files;
      // Drop the optimistic bubble again (it never reached the backend).
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

  /** Spawn an empty child subsession under a thread and jump into it. */
  async function newSubsession(parentId: string) {
    try {
      const m = await api.createSubsession({ parentId });
      await loadThreads();
      await openThread(m.id);
    } catch (e) {
      toast(String(e), true);
    }
  }

  /** Header agent: start (or continue) a thread with the Header role model. */
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
      });
      await loadThreads();
      await openThread(sid);
    } catch (e) {
      toast(String(e), true);
    }
  }

  /** Orchestrator: dispatch the first pending plan task as a lane worker. */
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
      case "clear":
        newThread();
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
        toast("/plan /auto /model /effort /new /clear /fork /subsession /kill /doctor");
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
        projectSelected = false;
      }
      toast(n > 0 ? `workspace ${target} deleted (${n} session${n === 1 ? "" : "s"})` : `workspace ${target} deleted`);
    } catch (e) {
      toast(String(e), true);
    }
  }

  /* ---------- Right inspector deck (Docs + Agents) ---------- */
  const RB_KEY = "parzi.rightbar";
  const rbSaved = (() => {
    try { return JSON.parse(localStorage.getItem(RB_KEY) ?? "{}") as { width?: number; auto?: boolean; tab?: string }; } catch { return {}; }
  })();
  // Closed on cold boot: the stage stays clean until something asks for it.
  let rightBarOpen = false;
  let rightBarTab: "docs" | "agents" = rbSaved.tab === "agents" ? "agents" : "docs";
  let rightBarWidth = Math.min(680, Math.max(340, Number(rbSaved.width) || 420));
  let autoReveal = rbSaved.auto ?? true;
  let selectedArtifact: InspectorArtifact | null = null;
  let selectedDoc: InspectorDoc | null = null;
  let projectDocs: DocEntry[] = [];
  let docLoading = false;
  let rbEl: HTMLElement | null = null;
  /** Last tool per session, fed by run events for every session (not just the open one). */
  let sessionTools: Record<string, { name: string; since: number; running: boolean }> = {};

  $: try { localStorage.setItem(RB_KEY, JSON.stringify({ width: rightBarWidth, auto: autoReveal, tab: rightBarTab })); } catch {}

  function openRightBar(tab: "docs" | "agents") {
    rightBarTab = tab;
    rightBarOpen = true;
  }
  function toggleRightBar(tab?: "docs" | "agents") {
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

  /** Every artifact version in the open thread, oldest first. */
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
      const picked = await openDialog({
        multiple: false,
        title: "Open a markdown file",
        defaultPath: currentRoot || undefined,
        filters: [{ name: "Markdown / text", extensions: ["md", "markdown", "txt", "mdx"] }],
      });
      if (typeof picked === "string" && picked) {
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

  /** Swarm = the whole tree the open thread belongs to (root + all descendants). */
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

  /** Re-fetch the open thread's persisted events without touching live text. */
  async function refreshEvents() {
    if (!activeThreadId) return;
    try {
      const [meta, ev] = await api.getThread(activeThreadId);
      activeMeta = meta;
      events = ev;
    } catch {}
  }

  async function approveFromDeck(key: string, allow: boolean) {
    try {
      await api.approveTool(key, allow);
      approval = null;
    } catch (e) {
      toast(String(e), true);
    }
  }

  function onEvent(e: UiEvent) {
    if (e.kind === "approval") {
      approval = { key: e.key, call: e.call };
      return;
    }
    if (e.kind === "subsession_created") {
      // A subsession spawned (by the user or an agent tool): refresh the
      // flat list so it shows up under its time bucket.
      loadThreads();
      if (autoReveal) openRightBar("agents");
      return;
    }
    if (e.kind === "tool_call") {
      sessionTools = { ...sessionTools, [e.session]: { name: e.name, since: Date.now(), running: true } };
    } else if (e.kind === "tool_result") {
      const cur = sessionTools[e.session];
      sessionTools = { ...sessionTools, [e.session]: { name: e.name, since: cur?.since ?? Date.now(), running: false } };
      // An artifact landed: pull it into the deck without resetting the live stream.
      if (e.name === "ui.show_artifact" && e.ok && e.session === activeThreadId) {
        refreshEvents().then(() => {
          const latest = threadArtifacts[threadArtifacts.length - 1];
          if (latest && autoReveal) showArtifact(latest);
        });
      }
    } else if (e.kind === "done" || e.kind === "error") {
      const { [e.session]: _gone, ...rest } = sessionTools;
      sessionTools = rest;
      // B5: a background session finishing must release the composer when it
      // was the live run. Handle per-session before the active-session gate.
      if (liveRun === e.session) {
        liveRun = null;
        // Only clear the streaming buffers when the finished session is the
        // one on screen; a background done must not wipe active live text.
        if (e.session === activeThreadId) {
          live = "";
          liveReasoning = "";
        }
      }
      // Per-session completion still refreshes the sidebar even for
      // background threads (handled below via the early return path).
    }
    if (e.session !== activeThreadId) {
      loadThreads();
      return;
    }
    if (e.kind === "text") live += e.text;
    else if (e.kind === "reasoning") liveReasoning += e.text;
    else if (e.kind === "notice") {
      toast(e.text);
      // Agent teamwork lands here (spawn/message notes): keep the tree live.
      loadThreads();
    }
    else if (e.kind === "usage") {
      liveTokens += e.tokens_in + e.tokens_out;
      liveCost += e.cost_usd;
    } else if (e.kind === "done" || e.kind === "error") {
      if (e.kind === "error") toast(e.error, true);
      liveRun = null;
      live = "";
      liveReasoning = "";
      loadThreads().then(() => {
        if (activeThreadId) openThread(activeThreadId);
      });
    } else {
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
    { section: "Actions", label: "New thread", sub: "start fresh", run: () => { palette = false; newThread(); } },
    { section: "Actions", label: "New project", sub: "workspace", run: () => { palette = false; openNewWs(); } },
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
      if (showNewWs) closeNewWs();
      else if (palette) palette = false;
      else if (showSettings) showSettings = false;
      else if (rightBarOpen && rbEl && rbEl.contains(document.activeElement)) rightBarOpen = false;
      else if (liveRun) stopRun();
    }
  }

  onMount(async () => {
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
    <div class="bg-overlay-dim" />
    <div class="bg-overlay-vignette" />
    <div class="bg-overlay-glow" />
  </div>

  <!-- Main View Split: sidebar runs full height, titlebar lives over the stage -->
  <div class="app-body">
    <div class="sb-wrap" class:closed={!sidebarOpen}>
    {#if showSettings}
    <!-- Settings mode: sidebar becomes section nav (T3-style) -->
    <div class="sb-pane" in:fly={{ x: -12, ...smooth }} out:fly={{ x: -8, ...smoothFast }}>
    <SettingsNav
      activeSection={settingsSection}
      authedCount={models.filter((m) => m.auth === "ok").length}
      on:select={(e) => { settingsSection = e.detail.id; }}
      on:back={() => (showSettings = false)}
    />
    </div>
    {:else}
    <!-- Clean Minimalist Sidebar -->
    <div class="sb-pane" in:fly={{ x: -12, ...smooth }} out:fly={{ x: -8, ...smoothFast }}>
    <Sidebar
      {projects}
      currentProject={curProject}
      {currentRoot}
      {branch}
      {threads}
      {activeThreadId}
      on:selectProject={(e) => switchProject(e.detail.name)}
      on:openNewWorkspace={openNewWs}
      on:selectThread={(e) => openThread(e.detail.id)}
      on:newSubsession={(e) => newSubsession(e.detail.id)}
      on:newThread={newThread}
      on:newThreadInProject={(e) => newThreadInProject(e.detail.name)}
      on:forkThread={(e) => fork(e.detail.id)}
      on:deleteThread={(e) => handleDeleteThread(e.detail.id)}
      on:deleteProject={(e) => handleDeleteProject(e.detail.name)}
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
    <!-- Central Stage (expand control lives inline in the titlebar) -->
    <div class="stage-col">
    <Titlebar
      title={showSettings ? "Settings" : curProject === "default" ? "Inbox" : curProject}
      subtitle={showSettings ? settingsSection : activeMeta ? activeMeta.title : ""}
      showExpand={!sidebarOpen}
      panelOpen={rightBarOpen}
      panelTab={rightBarTab}
      docsLoaded={!!(selectedArtifact || selectedDoc)}
      agentCount={swarmNodes.length}
      agentLive={liveAgentCount}
      on:expand={() => (sidebarOpen = true)}
      on:toggleDocs={() => toggleRightBar("docs")}
      on:toggleAgents={() => toggleRightBar("agents")}
      on:togglePanel={() => toggleRightBar()}
    />
    <main class="stage-container" class:settings-mode={showSettings}>
      {#if showSettings}
        <!-- Settings stage: main screen becomes the section (T3-style) -->
        <div class="stage-scroll settings-stage" in:fly={{ y: 12, ...smooth }} out:fly={{ y: 8, ...smoothFast }}>
          <Settings settingsTab={settingsSection} bareSection={settingsSection} currentProject={curProject} />
        </div>
      {:else if activeThreadId}
        <!-- Chat Thread View -->
        <div class="stage-scroll" bind:this={scrollEl}>
          <Thread {events} liveText={live} liveReasoning={liveReasoning} {approval} streaming={!!liveRun}
            {parentTitle} subsessions={childSubs} projectRoot={currentRoot}
            on:goParent={() => { if (activeMeta?.parent_id) openThread(activeMeta.parent_id); }}
            on:openSubsession={(e) => openThread(e.detail.id)}
            on:openArtifact={(e) => showArtifact(e.detail.artifact)}
            on:inspectSwarm={() => openRightBar("agents")} />
        </div>
        <div class="stage-omnibar-dock">
          <Omnibar
            bind:input
            bind:model
            bind:effort
            bind:mode
            bind:attachments
            streaming={!!liveRun}
            currentProject={curProject}
            currentTask={null}
            currentSubfolder={null}
            projectRoot={currentRoot}
            {branch}
            tokens={liveTokens}
            {models}
            on:send={send}
            on:stop={stopRun}
            on:modelChange={(e) => (model = e.detail.model)}
            on:command={(e) => handleBarCommand(e.detail.name)}
            on:openPlanner={openPlanner}
          />
        </div>
      {:else if projectSelected}
        <!-- Project Mission Control: roster + living plan + lane swarm -->
        <div class="stage-scroll">
          <ProjectMainPage
            project={curProject}
            root={currentRoot}
            {branch}
            lanes={currentProjectView?.lanes ?? []}
            {threads}
            roster={projectRoster}
            plan={projectPlan}
            on:openThread={(e) => openThread(e.detail.id)}
            on:newThread={newThread}
            on:killRun={(e) => killSession(e.detail.id)}
            on:newSubsession={(e) => newSubsession(e.detail.id)}
            on:askHeader={(e) => askHeader(e.detail.prompt)}
            on:executePlan={executePlan}
            on:planChanged={(e) => (projectPlan = e.detail.plan)}
            on:rosterSaved={(e) => (projectRoster = e.detail.roster)}
          />
        </div>
      {:else}
        <!-- Pure Minimalist Home Stage with Centered Floating Omnibar -->
        <div class="home-hero-stage">
          <Omnibar
            bind:input
            bind:model
            bind:effort
            bind:mode
            bind:attachments
            streaming={!!liveRun}
            currentProject={curProject}
            currentTask={null}
            currentSubfolder={null}
            projectRoot={currentRoot}
            {branch}
            tokens={liveTokens}
            {models}
            on:send={send}
            on:stop={stopRun}
            on:modelChange={(e) => (model = e.detail.model)}
            on:command={(e) => handleBarCommand(e.detail.name)}
            on:openPlanner={openPlanner}
          />
        </div>
      {/if}
    </main>
    </div>
    <!-- Right inspector deck: docked split, collapses to zero width -->
    <div class="rb-wrap" class:closed={!rightBarOpen} style="--rb-w: {rightBarWidth}px" bind:this={rbEl}>
      <RightPanel
        tab={rightBarTab}
        width={rightBarWidth}
        {autoReveal}
        artifact={selectedArtifact}
        artifacts={threadArtifacts}
        doc={selectedDoc}
        docs={projectDocs}
        {docLoading}
        nodes={swarmNodes}
        {activeThreadId}
        {events}
        {approval}
        on:close={() => (rightBarOpen = false)}
        on:tab={(e) => (rightBarTab = e.detail.tab)}
        on:resize={(e) => (rightBarWidth = e.detail.width)}
        on:autoReveal={(e) => (autoReveal = e.detail.on)}
        on:openDoc={(e) => openProjectDoc(e.detail.entry)}
        on:openTranscript={openTranscript}
        on:openArtifact={(e) => showArtifact(e.detail.artifact)}
        on:pickFile={pickDocFile}
        on:focus={(e) => openThread(e.detail.id)}
        on:fork={(e) => fork(e.detail.id)}
        on:kill={(e) => killSession(e.detail.id)}
        on:spawn={(e) => newSubsession(e.detail.id)}
        on:approve={(e) => approveFromDeck(e.detail.key, e.detail.allow)}
      />
    </div>
  </div>

  <!-- New Workspace Modal -->
  <!-- Toast Notification Stack -->
  <div class="toast-stack">
    {#each toasts as t (t.id)}
      <div class="toast-item" class:error-toast={t.err} transition:fade|local={{ duration: 140 }}>
        {t.text}
      </div>
    {/each}
  </div>

  <!-- New Workspace Popout (centered over the backdrop) -->
  {#if showNewWs}
    <div class="ws-backdrop" transition:fade={{ duration: 140 }} on:click={closeNewWs}>
      <div class="ws-modal" on:click|stopPropagation>
        <div class="ws-head">
          <span>New workspace</span>
          <button class="ws-x" title="Cancel (Esc)" on:click={closeNewWs}>
            <Icon d="M18 6L6 18M6 6l12 12" size={12} />
          </button>
        </div>
        <input
          class="ws-name"
          placeholder="Name — e.g. strohmann"
          bind:value={wsName}
          use:focusWsName
          on:keydown={(e) => { if (e.key === "Enter") submitNewWs(); }}
        />
        <button
          class="ws-folder"
          on:click={browseWsFolder}
          disabled={pickingWsFolder}
          title={wsRoot.trim() || "Choose a workspace folder (optional)"}
        >
          <Icon d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" size={13} />
          <span class="ws-folder-label">{wsFolderLabel || "Folder (optional)"}</span>
        </button>
        <div class="ws-foot">
          <span class="hint">Enter to create · Esc to cancel</span>
          <button class="ws-create" on:click={submitNewWs} disabled={!wsName.trim()}>Create</button>
        </div>
      </div>
    </div>
  {/if}

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
  /* Visible text highlighting for copy (was invisible: no rule + body none). */
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
    /* Frameless transparent window: soft 10px corners so no edge cuts hard
       against the desktop. `.maximized` (see onMount) drops it to square. */
    border-radius: 10px;
  }
  /* Maximized state arrives on <html> at runtime (see onMount), so the
     selector lives behind :global to keep svelte-check quiet. */
  :global(html.parzi-maximized) .parzi-app-shell {
    border-radius: 0;
  }
  /* Wallpaper stack: picture → stage-tinted dim → vignette → faint accent glow.
     Every layer follows the theme; with no picture the stage colour shows. */
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
    filter: blur(var(--parzi-bg-blur)) saturate(1.05);
    transform: scale(1.04);
  }
  .bg-overlay-dim {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--stage) var(--parzi-bg-dim-pct), transparent);
  }
  .bg-overlay-vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse at 55% 42%, transparent 30%, rgba(0, 0, 0, var(--parzi-vignette)) 100%);
  }
  .bg-overlay-glow {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse at 50% 10%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 60%);
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
  /* Soft divider: a 24px gradient breathes the sidebar into the stage
     instead of a hard 1px cut. Lives on the wrap so it covers both the
     workspace sidebar and the settings nav. */
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
  /* Panes overlay so the settings/sidebar crossfade never changes layout width. */
  .sb-pane { position: absolute; inset: 0; width: 248px; display: flex; flex-direction: column; min-height: 0; will-change: transform, opacity; }
  /* Right inspector deck: mirrors the sidebar slide. Width is a CSS var so a
     drag-resize stays a style change, not a re-layout of the whole shell. */
  .rb-wrap {
    flex: none; width: var(--rb-w, 420px); margin-right: 0; opacity: 1; position: relative;
    min-height: 0; display: flex;
    transition: margin-right 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 0s;
  }
  /* visibility flips after the slide so the deck is untabbable once hidden. */
  .rb-wrap.closed {
    margin-right: calc(-1 * var(--rb-w, 420px)); opacity: 0; pointer-events: none; visibility: hidden;
    transition: margin-right 260ms cubic-bezier(0.22, 1, 0.36, 1), opacity 200ms ease, visibility 0s linear 260ms;
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
  .stage-omnibar-dock {
    padding: 12px 24px 20px;
    display: flex;
    justify-content: center;
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
  /* Runs + dialogs removed: creation and run control live in the sidebar. */

  /* Toast Stack */
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

  /* New-workspace popout */
  .ws-backdrop {
    position: fixed; inset: 0; z-index: 250;
    display: flex; align-items: center; justify-content: center;
    background: color-mix(in srgb, var(--stage) 55%, transparent);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
  }
  .ws-modal {
    width: 340px; max-width: calc(100vw - 48px);
    display: flex; flex-direction: column; gap: 10px;
    padding: 16px 16px 12px;
    background: var(--menu);
    border: 1px solid var(--line-3);
    border-radius: 14px;
    box-shadow: var(--menu-shadow);
  }
  .ws-head {
    display: flex; align-items: center; justify-content: space-between;
    font-size: 13px; font-weight: 600; color: var(--text);
  }
  .ws-x {
    display: inline-flex; align-items: center; justify-content: center;
    width: 24px; height: 24px; background: transparent; border: none;
    border-radius: 6px; color: var(--text-3); cursor: pointer;
  }
  .ws-x:hover { background: var(--surface-3); color: var(--text); }
  .ws-name {
    background: var(--input); border: 1px solid var(--line-3);
    border-radius: 8px; color: var(--text); font: inherit; font-size: 13px;
    padding: 9px 11px; outline: none; width: 100%; box-sizing: border-box;
  }
  .ws-name::placeholder { color: var(--text-4); }
  .ws-name:focus { border-color: var(--accent-line) !important; background: var(--surface-2); box-shadow: none; }
  .ws-folder {
    display: flex; align-items: center; gap: 8px; width: 100%; box-sizing: border-box;
    background: transparent; border: 1px dashed var(--line-3);
    border-radius: 8px; color: var(--text-3); font: inherit; font-size: 12.5px;
    padding: 8px 11px; cursor: pointer; text-align: left;
  }
  .ws-folder:hover:not(:disabled) { color: var(--text-2); border-color: var(--text-4); background: var(--surface-1); }
  .ws-folder:disabled { opacity: 0.5; cursor: default; }
  .ws-folder-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ws-foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .ws-foot .hint { font-size: 10.5px; color: var(--text-4); }
  .ws-create {
    background: var(--accent); border: 1px solid transparent;
    color: var(--accent-ink); font: inherit; font-size: 12.5px; font-weight: 600;
    padding: 7px 18px; cursor: pointer; border-radius: 8px;
  }
  .ws-create:hover:not(:disabled) { filter: brightness(1.08); }
  .ws-create:disabled { opacity: 0.4; cursor: default; }
</style>
