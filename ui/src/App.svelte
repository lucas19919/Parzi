<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api, brain, deskSync, onBrowser, onBrowserKey, onBrowserOpen, onDesk, onRunEvent,
    MODE_META, type ChatEvent, type ComposerMode, type PageEvent, type Question, type SessionMeta, type UiEvent,
  } from "./lib/api";
  import TopBar from "./lib/TopBar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import WinControls from "./lib/WinControls.svelte";
  import { startWindowDrag } from "./lib/windowChrome";
  import Switcher from "./lib/Switcher.svelte";
  import DefaultArt from "./lib/DefaultArt.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import PanelHeader from "./lib/PanelHeader.svelte";
  import SidePanel from "./lib/SidePanel.svelte";
  import Thread from "./lib/Thread.svelte";
  import SessionHeader from "./lib/SessionHeader.svelte";
  import PageView from "./lib/PageView.svelte";
  import HistoryView from "./lib/HistoryView.svelte";
  import HomeView from "./lib/HomeView.svelte";
  import PreviewView from "./lib/PreviewView.svelte";
  import BrainView from "./lib/BrainView.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import { loadTabs, onboarded, recordVisit, saveTabs, titleVisit } from "./lib/browserData";
  import Settings from "./lib/Settings.svelte";
  import Icon from "./lib/Icon.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce } from "./lib/threadList";
  import { checkForUpdatesSoon } from "./lib/updateStore";
  import { brainTab, historyTab, hostOf, isExplicitUrl, pageTab, previewTab, sessionTab, settingsTab, toAddress, type Tab } from "./lib/tabs";
  import { openBrainNote, brainTabRequested } from "./lib/brainStore";
  import { toast, toastError, toasts, notify, chime } from "./lib/toast";
  import { covered } from "./lib/overlay";
  import type { Approval, LiveTool } from "./lib/live";

  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motion = reduced ? { duration: 0 } : { duration: 200, easing: cubicOut };

  const restored = loadTabs();
  let tabs: Tab[] = (restored?.tabs ?? [sessionTab()]).map((t) => {
    const c = (t as Partial<Tab>).composer ?? { input: "", mode: "build", model: "", effort: "medium", attachments: [] };
    if (c.model === "auto") c.model = "";
    return { ...t, composer: c };
  });
  let activeId = restored?.active ?? tabs[0].id;
  // In-tab subagent navigation: when the panel opens a descendant of the
  // visible session, we swap the tab's session in place and remember the
  // way back instead of spawning another top-level tab.
  let navTrail: string[] = [];
  let navTab = "";
  let threads: SessionMeta[] = [];
  let bg = "";

  let shown: string | null = null;
  let meta: SessionMeta | null = null;
  let events: ChatEvent[] = [];
  let running = new Set<string>();
  let approvals: Approval[] = [];
  let questions: Question[] = [];
  let context = { used: 0, limit: 0 };
  let compacting = false;
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTools: LiveTool[] = [];
  let pending = "";
  let flushTimer = 0;

  let input = "";
  let model = "";
  let warmed = "";
  let effort = "medium";
  let permission = "full";
  let mode: ComposerMode = "build";
  let attachments: string[] = [];
  let sending = false;

  // Omnibar binds directly to the active tab's composer (see markup
  // below), so each tab keeps its own draft, mode, model, effort and
  // attachments without cross-contamination.

  function laneOf(sessionId: string | null | undefined): string {
    if (!sessionId) return "";
    return threads.find((t) => t.id === sessionId)?.lane ?? "";
  }

  // Legacy sessions stored lane "code" (today's "build") or "research"
  // (today's "work").
  function normLane(lane: string): string {
    if (lane === "code") return "build";
    if (lane === "research") return "work";
    return lane;
  }

  function isDescendant(id: string, ancestor: string): boolean {
    let cur = threads.find((t) => t.id === id)?.parent_id;
    let guard = 0;
    while (cur && guard++ < 50) {
      if (cur === ancestor) return true;
      cur = threads.find((t) => t.id === cur)?.parent_id;
    }
    return false;
  }

  $: familyActive = (() => {
    if (!shown) return 0;
    const focus: string = shown;
    return threads.filter(
      (t) => t.id !== focus && isDescendant(t.id, focus) && (t.status === "active" || t.status === "queued" || running.has(t.id)),
    ).length;
  })();

  // Composer state lives on the tab: switching tabs preserves each
  // tab's composer intact, with no copying and no cross-contamination.

  let switcherOpen = false;
  let panelOpen = false;
  let panelW = 360;
  let panelTab: "agents" | "projects" | "tasks" = "agents";
  let settingsSection = "general";
  let scrollEl: HTMLElement | null = null;
  let farFromBottom = false;
  let dockHeight = 90;
  let omnibar: Omnibar;
  let page: PageView;
  let immersive = false;
  let layout: "topbar" | "sidebar" = "topbar";
  let setupOpen = false;
  let project: { slug: string; title: string; tokens: number } | null = null;
  let navSeq = 0;
  let deskRev = 0;

  $: tab = tabs.find((t) => t.id === activeId) ?? tabs[0];
  $: composer = tab.composer;
  // Legacy mirrors for session logic; the Omnibar binds directly to
  // tab.composer.* (see markup), so each tab keeps its own state.
  $: input = composer.input;
  $: mode = composer.mode;
  $: model = composer.model;
  $: effort = composer.effort;
  $: attachments = composer.attachments;
  $: streaming = !!shown && running.has(shown);
  $: hasSession = tab.kind === "session" && (!!shown || sending || events.length > 0);
  $: folder = tab.kind === "session" && tab.sessionId ? (meta?.cwd ?? "") : (tab.cwd ?? "");
  $: approval = approvals.find((a) => a.session === shown) ?? null;
  $: openQuestion = questions.find((q) => q.session === shown) ?? null;
  $: lockMode = (() => {
    if (!hasSession) return null;
    const l = normLane(meta?.lane ?? "");
    return l === "build" || l === "work" ? (l as ComposerMode) : null;
  })();
  $: sessionLanes = Object.fromEntries(threads.map((t) => [t.id, t.lane ?? ""]));
  $: void loadBranch(folder);
  $: void loadProject(folder);
  $: syncDesk(tabs, activeId);
  $: saveTabs(tabs, activeId);
  $: pageVisible = tab.kind === "page";
  $: if (!pageVisible) api.browserHide().catch(() => {});
  $: if (!pageVisible && immersive) void setImmersive(false);

  const refreshThreads = coalesce(async () => {
    threads = await api.listThreads().catch(() => threads);
  }, 100);

  function patchTab(id: string, patch: Partial<Tab>) {
    tabs = tabs.map((t) => (t.id === id ? { ...t, ...patch } : t));
  }

  function syncDesk(list: Tab[], active: string) {
    const rows = list.map((t) => ({
      id: t.id,
      kind: t.kind === "page" ? ("browser" as const) : t.kind === "brain" || t.kind === "history" ? t.kind : ("harness" as const),
      title: t.title,
      url: t.url ?? "",
      session_id: t.sessionId ?? "",
    }));
    deskSync(rows, deskRev, active).catch(() => {});
  }

  async function loadProject(dir: string) {
    const ctx = dir ? await brain.context(dir).catch(() => null) : null;
    project = ctx?.project ? { slug: ctx.project.slug, title: ctx.project.title, tokens: ctx.tokens } : null;
  }

  let closed: { url: string; title: string }[] = [];

  let historyView: "sessions" | "history" | "bookmarks" = "sessions";

  function openHistory(view = historyView) {
    historyView = view;
    const existing = tabs.find((t) => t.kind === "history");
    if (existing) selectTab(existing.id);
    else addTab(historyTab());
  }

  function reopenClosed() {
    const last = closed[closed.length - 1];
    if (!last) return;
    closed = closed.slice(0, -1);
    openPageNext(last.url);
  }

  function openBrain(path: string | unknown = "") {
    if (typeof path === "string" && path) openBrainNote(path);
    const existing = tabs.find((t) => t.kind === "brain");
    if (existing) selectTab(existing.id);
    else addTab(brainTab());
  }

  async function loadBranch(dir: string) {
    branch = dir ? await api.gitBranch(dir).catch(() => "") : "";
  }

  function clearLive() {
    clearTimeout(flushTimer);
    flushTimer = 0;
    pending = "";
    live = "";
    liveReasoning = "";
    liveTools = [];
  }

  function nearBottom() {
    return !!scrollEl && scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight < 160;
  }

  async function scrollToBottom(smooth = false) {
    await tick();
    scrollEl?.scrollTo({ top: scrollEl.scrollHeight, behavior: smooth && !reduced ? "smooth" : "auto" });
  }

  function showDraft() {
    navSeq++;
    shown = null;
    meta = null;
    events = [];
    context = { used: 0, limit: 0 };
    clearLive();
  }

  $: warm(model.split("/")[0]);
  $: warm(meta?.model.split("/")[0] ?? "");

  function warm(provider: string) {
    if (!provider || provider === "auto" || provider === warmed) return;
    warmed = provider;
    void api.warmAgent(provider).catch(() => {});
  }

  async function showSession(id: string) {
    const my = ++navSeq;
    shown = id;
    clearLive();
    try {
      const [m, ev] = await api.getThread(id);
      if (my !== navSeq) return;
      meta = m;
      events = ev;
      context = { used: m.context_tokens ?? 0, limit: m.context_limit ?? 0 };
      const comp = tab.composer;
      if (m.model.includes("/")) {
        comp.model = m.model;
        tabs = tabs;
      }
      if (m.status === "active" || m.status === "queued") running = new Set(running).add(id);
      if (tab.sessionId === id && m.title) patchTab(tab.id, { title: m.title });
      await scrollToBottom();
    } catch (e) {
      if (my === navSeq) toastError(e);
    }
  }

  async function reload() {
    if (!shown) return;
    const id = shown;
    const my = navSeq;
    const [m, ev] = await api.getThread(id).catch(() => [null, null] as const);
    if (!m || !ev || my !== navSeq || shown !== id) return;
    meta = m;
    events = ev;
    context = { used: m.context_tokens ?? context.used, limit: m.context_limit ?? context.limit };
  }

  async function setImmersive(on: boolean) {
    if (immersive === on) return;
    immersive = on;
    document.documentElement.classList.toggle("parzi-maximized", on);
    await api.windowFullscreen(on).catch(() => {});
  }

  function selectTab(id: string) {
    const next = tabs.find((t) => t.id === id);
    if (!next) return;
    // Each tab owns its composer; switching only changes the active tab
    // so drafts, mode, model, effort and attachments are preserved intact.
    activeId = id;
    if (navTab !== id) {
      navTrail = [];
      navTab = "";
    }
    if (next.kind === "session") {
      if (next.sessionId) void showSession(next.sessionId);
      else showDraft();
    }
  }

  function addTab(t: Tab) {
    if (!t.composer) t.composer = { input: "", mode: "build", model: "", effort: "medium", attachments: [] };
    tabs = [...tabs, t];
    selectTab(t.id);
  }

  function moveTab(id: string, to: number) {
    const from = tabs.findIndex((t) => t.id === id);
    if (from < 0 || from === to) return;
    const next = [...tabs];
    const [moved] = next.splice(from, 1);
    next.splice(Math.max(0, Math.min(to, next.length)), 0, moved);
    tabs = next;
  }

  function closeTab(id: string) {
    const idx = tabs.findIndex((t) => t.id === id);
    if (idx < 0) return;
    const closedKind = tabs[idx].kind;
    // Closing a session tab takes its agent-owned pages with it.
    const doomed = new Set([id]);
    const closingSession = tabs[idx].kind === "session" ? tabs[idx].sessionId : null;
    for (const t of tabs) {
      if (t.kind === "page" && t.owner && closingSession && t.owner === closingSession) doomed.add(t.id);
    }
    for (const dead of doomed) {
      const t = tabs.find((x) => x.id === dead);
      if (t?.kind === "page") {
        api.browserClose(dead).catch(() => {});
        if (t.url) closed = [...closed, { url: t.url, title: t.title }].slice(-20);
      }
    }
    if (settingsReturn && doomed.has(settingsReturn)) settingsReturn = null;
    if (navTab === id || doomed.has(navTab)) {
      navTrail = [];
      navTab = "";
    }
    const kept = tabs.filter((t) => !doomed.has(t.id));
    if (!kept.length) {
      const fresh = sessionTab();
      tabs = [fresh];
      selectTab(tabs[0].id);
      return;
    }
    tabs = kept;
    if (!doomed.has(activeId)) return;
    // Settings (and friends) return to the tab you came from, not a neighbor.
    if (closedKind === "settings" && settingsReturn && kept.some((t) => t.id === settingsReturn)) {
      const back = settingsReturn;
      settingsReturn = null;
      selectTab(back);
      return;
    }
    selectTab(kept[Math.max(0, Math.min(idx, kept.length - 1))].id);
  }

  function newSession() {
    addTab({ ...sessionTab(), cwd: folder || undefined });
    void tick().then(() => omnibar?.focus());
  }

  function goHome() {
    const draft = tabs.find((t) => t.kind === "session" && !t.sessionId);
    if (draft) selectTab(draft.id);
    else newSession();
  }

  function openPage(url: string, id?: string, owner?: string | null, background = false) {    const existing = tabs.find((t) => (id ? t.id === id : t.kind === "page" && t.url === url));
    if (existing) {
      if (url && existing.url !== url) patchTab(existing.id, { url, title: hostOf(url) });
      if (owner !== undefined && existing.owner !== owner) patchTab(existing.id, { owner: owner ?? null });
      if (!background) selectTab(existing.id);
    } else {
      const t = pageTab(url, id, owner ?? null);
      if (background) {
        const idx = tabs.findIndex((x) => x.id === activeId);
        tabs = [...tabs.slice(0, idx + 1), t, ...tabs.slice(idx + 1)];
      } else {
        addTab(t);
      }
    }
  }

  function openPageNext(url: string) {
    const t = pageTab(url);
    const idx = tabs.findIndex((x) => x.id === activeId);
    tabs = [...tabs.slice(0, idx + 1), t, ...tabs.slice(idx + 1)];
    selectTab(t.id);
  }

  function navigatePage(url: string) {
    const id = tab.id;
    patchTab(id, { url, title: hostOf(url) });
    api.browserNavigate(id, url).catch(() => {});
  }

  const pendingTitles = new Map<string, string>();

  function onPage(e: PageEvent) {
    const t = tabs.find((x) => x.id === e.tab);
    if (!t) return;
    const patch: Partial<Tab> = {};
    if (e.loading !== undefined) patch.loading = e.loading;
    if (e.canGoBack !== undefined) patch.canGoBack = e.canGoBack;
    if (e.canGoForward !== undefined) patch.canGoForward = e.canGoForward;
    if (e.blocked !== undefined) patch.blocked = e.blocked;
    if (e.bg) patch.bg = e.bg;
    if (e.title) {
      patch.title = e.title.slice(0, 80);
      if (t.loading) pendingTitles.set(e.tab, e.title);
      else if (t.url) titleVisit(t.url, e.title);
    }
    if (e.url) {
      const pending = pendingTitles.get(e.tab);
      patch.url = e.url;
      if (e.url !== t.url && !e.title) patch.title = (pending ?? hostOf(e.url)).slice(0, 80);
      if (e.loading === false) {
        recordVisit(e.url, pending ?? "");
        pendingTitles.delete(e.tab);
      }
    }
    patchTab(e.tab, patch);
    if (e.fullscreen !== undefined && e.tab === activeId) void setImmersive(e.fullscreen);
  }

  function browse(url: string) {
    if (tab.kind === "session" && !tab.sessionId && !events.length) {
      patchTab(tab.id, { kind: "page", url, title: hostOf(url), sessionId: null });
    } else {
      openPage(url);
    }
  }

  function openSession(id: string) {
    // A descendant of the visible session opens in place (same tab) with
    // a way back; anything else keeps the old find-or-open-tab behavior.
    if (shown && id !== shown && isDescendant(id, shown) && tab.kind === "session") {
      navTrail = [...navTrail, shown];
      navTab = tab.id;
      patchTab(tab.id, { sessionId: id, title: threads.find((t) => t.id === id)?.title || "Session" });
      void showSession(id);
      return;
    }
    const existing = tabs.find((t) => t.sessionId === id);
    if (existing) {
      selectTab(existing.id);
      return;
    }
    const title = threads.find((t) => t.id === id)?.title || "Session";
    if (tab.kind === "session" && !tab.sessionId && !events.length) {
      patchTab(tab.id, { sessionId: id, title });
      selectTab(tab.id);
    } else {
      addTab(sessionTab(id, title));
    }
  }

  function goBack() {
    const prev = navTrail[navTrail.length - 1];
    if (!prev) return;
    navTrail = navTrail.slice(0, -1);
    const host = tabs.find((t) => t.id === navTab);
    if (host && host.kind === "session") {
      patchTab(host.id, { sessionId: prev, title: threads.find((t) => t.id === prev)?.title || "Session" });
      if (host.id === activeId) void showSession(prev);
    } else {
      openSession(prev);
    }
    if (!navTrail.length) navTab = "";
  }

  $: backTitle = navTrail.length && navTab === tab.id ? (threads.find((t) => t.id === navTrail[navTrail.length - 1])?.title || "Back") : null;

  function openPreview(which = "omnibar") {
    const existing = tabs.find((t) => t.kind === "preview" && t.url === which);
    if (existing) selectTab(existing.id);
    else addTab(previewTab(which));
  }

  // Closing settings always lands back where you came from.
  let settingsReturn: string | null = null;

  function openSettings(section = "general") {
    settingsSection = section;
    switcherOpen = false;
    const existing = tabs.find((t) => t.kind === "settings");
    if (existing) {
      selectTab(existing.id);
      return;
    }
    if (tab.kind !== "settings") settingsReturn = activeId;
    addTab(settingsTab());
  }

  async function send() {
    const target = tab;
    const comp = target.composer;
    const prompt = comp.input.trim();
    if (!prompt || sending || streaming) return;
    if (!comp.model && comp.mode !== "search") {
      toast("Pick a model first");
      omnibar?.openModels();
      return;
    }
    if (isExplicitUrl(prompt)) {
      comp.input = "";
      tabs = tabs;
      browse(toAddress(prompt));
      return;
    }
    const files = [...comp.attachments];
    const sessionLane = normLane(laneOf(target.sessionId));
    const fresh = !target.sessionId || (sessionLane !== "" && sessionLane !== comp.mode);
    if (fresh) {
      navTrail = [];
      navTab = "";
    }
    const optimistic: ChatEvent = { kind: "user", text: prompt };
    sending = true;
    comp.input = "";
    comp.attachments = [];
    tabs = tabs;
    events = [...events, optimistic];
    try {
      const sid = await api.sendMessage({
        sessionId: fresh ? null : target.sessionId,
        model: comp.model,
        prompt,
        cwd: folder,
        effort: comp.effort,
        attachments: files,
        mode: permission,
        lane: comp.mode,
      });
      running = new Set(running).add(sid);
      // The field was cleared before the await. Drop the stored draft
      // for the tab that sent, not whichever tab is active now.
      target.composer.input = "";
      target.composer.attachments = [];
      tabs = tabs;
      if (fresh) patchTab(target.id, { sessionId: sid, title: prompt.slice(0, 40) });
      if (activeId === target.id) {
        shown = sid;
        await reload();
        await scrollToBottom();
      }
      void refreshThreads();
    } catch (e) {
      target.composer.input = prompt;
      target.composer.attachments = files;
      tabs = tabs;
      events = events.filter((ev) => ev !== optimistic);
      toastError(e);
    } finally {
      sending = false;
    }
  }

  async function stop() {
    if (!shown || !running.has(shown)) return;
    const id = shown;
    running.delete(id);
    running = running;
    clearLive();
    await reload();
    void refreshThreads();
    await stopSession(id);
  }

  async function stopSession(id: string) {
    try {
      await api.killRun(id);
      running.delete(id);
      running = running;
      void refreshThreads();
    } catch (e) {
      toastError(e);
    }
  }

  async function fork() {
    if (!shown) return;
    try {
      const m = await api.forkThread(shown);
      await refreshThreads();
      addTab(sessionTab(m.id, m.title || "Fork"));
      toast("Forked");
    } catch (e) {
      toastError(e);
    }
  }

  async function compact() {
    if (!shown) {
      toast("Nothing to compact yet");
      return;
    }
    compacting = true;
    let timer = 0;
    try {
      await Promise.race([
        (async () => {
          await api.compactThread(shown);
          await reload();
        })(),
        new Promise<never>((_, reject) => {
          timer = window.setTimeout(
            () => reject(new Error("Compact timed out — the summarizer is taking too long. You can try again.")),
            300_000,
          );
        }),
      ]);
    } catch (e) {
      toastError(e);
    } finally {
      clearTimeout(timer);
      compacting = false;
    }
  }

  async function rename(title: string) {
    if (!shown) return;
    const id = shown;
    try {
      await api.renameThread(id, title);
      if (meta?.id === id) meta = { ...meta, title };
      tabs = tabs.map((t) => (t.sessionId === id ? { ...t, title } : t));
      void refreshThreads();
    } catch (e) {
      toastError(e);
    }
  }

  async function remove(id: string) {
    const title = threads.find((t) => t.id === id)?.title || meta?.title || "this session";
    const ok = await ask(`Delete "${title}" and its transcript?`, { title: "Delete session", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await api.deleteThread(id);
      running.delete(id);
      running = running;
      const open = tabs.filter((t) => t.sessionId === id).map((t) => t.id);
      for (const tid of open) closeTab(tid);
      void refreshThreads();
      toast("Session deleted");
    } catch (e) {
      toastError(e);
    }
  }

  function copyId() {
    if (!shown) return;
    navigator.clipboard.writeText(shown).catch(() => {});
    toast("Session ID copied");
  }

  // History's Clear sessions: purge finished threads, close their tabs.
  // Running threads are left alone.
  async function clearFinishedSessions() {
    const idle = threads.filter((t) => !running.has(t.id));
    if (!idle.length) {
      toast("Nothing to clear");
      return;
    }
    const ok = await ask(`Delete ${idle.length} finished session${idle.length === 1 ? "" : "s"}? Running ones stay.`, {
      title: "Clear sessions",
      kind: "warning",
    }).catch(() => false);
    if (!ok) return;
    try {
      const count = await api.purgeSessions();
      await refreshThreads();
      const gone = new Set(threads.map((t) => t.id));
      for (const t of tabs.filter((t) => t.sessionId && !gone.has(t.sessionId))) closeTab(t.id);
      toast(count ? `Cleared ${count} sessions` : "Sessions cleared");
    } catch (e) {
      toastError(e);
    }
  }

  function onCommand(name: string) {
    if (name === "new") newSession();
    else if (name === "fork") void fork();
    else if (name === "compact") void compact();
    else if (name === "stop") void stop();
    else if (name === "page") addTab(pageTab());
    else if (name === "settings") openSettings();
  }

  function flushLive() {
    flushTimer = 0;
    if (!pending) return;
    const follow = nearBottom();
    live += pending;
    pending = "";
    if (follow) void scrollToBottom();
  }

  function onEvent(e: UiEvent) {
    if (e.kind === "approval") {
      approvals = [...approvals.filter((a) => a.key !== e.key), { key: e.key, session: e.session, call: e.call }];
      return;
    }
    if (e.kind === "question") {
      questions = [...questions.filter((q) => q.key !== e.key), { key: e.key, session: e.session, question: e.question, options: e.options }];
      return;
    }
    if (e.kind === "done" || e.kind === "error") {
      running.delete(e.session);
      running = running;
      approvals = approvals.filter((a) => a.session !== e.session);
      questions = questions.filter((q) => q.session !== e.session);
      void refreshThreads();
      if (e.session === shown) {
        flushLive();
        clearLive();
        void reload();
      } else {
        const title = threads.find((t) => t.id === e.session)?.title || "Agent";
        notify(e.kind === "done" ? `${title} finished` : `${title} stopped`, e.session);
        chime(e.kind === "done");
      }
      return;
    }
    if (!running.has(e.session) && e.kind !== "context") {
      running = new Set(running).add(e.session);
    }
    if (e.session !== shown) return;
    if (e.kind === "text") {
      pending += e.text;
      if (!flushTimer) flushTimer = window.setTimeout(() => requestAnimationFrame(flushLive), 60);
    } else if (e.kind === "reasoning") {
      liveReasoning += e.text;
    } else if (e.kind === "tool_call") {
      if (!liveTools.some((t) => t.id === e.id)) {
        liveTools = [...liveTools, { id: e.id, name: e.name, label: e.label || e.name, running: true, ok: true, ms: 0 }];
      }
    } else if (e.kind === "tool_result") {
      liveTools = liveTools.map((t) => (t.running && t.id === e.id ? { ...t, running: false, ok: e.ok, ms: e.ms } : t));
    } else if (e.kind === "context") {
      context = { used: e.used, limit: e.limit };
    }
  }

  function shortcut(name: string): boolean {
    if (name === "ctrl+p" || name === "ctrl+k") switcherOpen = !switcherOpen;
    else if (name === "ctrl+t") newSession();
    else if (name === "ctrl+w") closeTab(activeId);
    else if (name === "ctrl+comma") tab.kind === "settings" ? closeTab(activeId) : openSettings();
    else if (name === "ctrl+b") openBrain();
    else if (name === "ctrl+h") openHistory();
    else if (name === "ctrl+shift+a") {
      panelTab = "agents";
      panelOpen = true;
    }
    else if (name === "ctrl+shift+t") reopenClosed();
    else if (name === "f11") void setImmersive(!immersive);
    else if (name === "ctrl+l") {
      if (tab.kind === "page") {        void setImmersive(false);
        void page?.focusAddress();
      } else {
        mode = "search";
        omnibar?.focus();
      }
    } else if (name === "ctrl+tab" || name === "ctrl+shift+tab") {
      const idx = tabs.findIndex((t) => t.id === activeId);
      selectTab(tabs[(idx + (name === "ctrl+tab" ? 1 : -1) + tabs.length) % tabs.length].id);
    } else return false;
    return true;
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    const name =
      key === "f11"
        ? "f11"
        : mod && key === "tab"
          ? e.shiftKey
            ? "ctrl+shift+tab"
            : "ctrl+tab"
          : mod && key === ","
            ? "ctrl+comma"
          : mod && e.shiftKey && key === "t"
            ? "ctrl+shift+t"
            : mod && e.shiftKey && key === "a"
              ? "ctrl+shift+a"
              : mod && ["p", "k", "t", "w", "l", "b", "h"].includes(key)
                  ? `ctrl+${key}`
                  : "";
    if (name && shortcut(name)) {
      e.preventDefault();
    } else if (key === "escape" && !e.defaultPrevented && !$covered) {
      if (immersive) void setImmersive(false);
      else if (tab.kind === "settings") closeTab(activeId);
      else if (streaming) void stop();
    }
  }

  async function warmPages() {
    await new Promise((r) => setTimeout(r, 1500));
    const later = tabs.filter((t) => t.kind === "page" && t.url && t.id !== activeId).slice(0, 8);
    for (const t of later) {
      if (!tabs.some((x) => x.id === t.id)) continue;
      await api.browserPrepare(t.id, t.url ?? "").catch(() => {});
      await new Promise((r) => setTimeout(r, 500));
    }
  }

  onMount(() => {
    try {
      const saved = Number(localStorage.getItem("parzi.dock.w"));
      if (saved >= 280 && saved <= 640) panelW = saved;
    } catch {}
    try {
      layout = localStorage.getItem("parzi.layout") === "sidebar" ? "sidebar" : "topbar";
    } catch {}
    document.documentElement.classList.toggle("parzi-sidebar", layout === "sidebar");
    const onLayout = (e: Event) => {
      layout = (e as CustomEvent<string>).detail === "sidebar" ? "sidebar" : "topbar";
      document.documentElement.classList.toggle("parzi-sidebar", layout === "sidebar");
    };
    document.addEventListener("parzi:layout", onLayout);    const unRun = onRunEvent(onEvent);
    const unDesk = onDesk((cmd) => {
      if (typeof cmd.rev === "number") deskRev = Math.max(deskRev, cmd.rev);
      // Agent-driven tab events never steal focus: pages open in the
      // background attached to their session; navigations patch in place.
      if (cmd.op === "open" && cmd.url) openPage(cmd.url, cmd.id, cmd.owner ?? null, true);
      else if (cmd.op === "navigate" && cmd.id && cmd.url) {
        patchTab(cmd.id, { url: cmd.url, title: hostOf(cmd.url) });
      } else if (cmd.op === "focus" && cmd.id) selectTab(cmd.id);
      else if (cmd.op === "close" && cmd.id) closeTab(cmd.id);
    });
    const unPage = onBrowser(onPage);
    const unOpen = onBrowserOpen((e) => openPageNext(e.url));
    const unKey = onBrowserKey((name) => void shortcut(name));
    let brainSeen = 0;
    const unBrain = brainTabRequested.subscribe((n) => {
      if (n > 0 && n !== brainSeen) {
        brainSeen = n;
        openBrain();
      }
    });
    selectTab(activeId);
    if (!$onboarded) setupOpen = true;
    void warmPages();
    const onBg = (e: Event) => (bg = (e as CustomEvent<string>).detail);
    document.addEventListener("parzi:bg", onBg);
    (async () => {
      try {
        applyThemeCss(await api.getThemeCss());
        bg = await api.backgroundUrl();
        await ensureBoard();
        await refreshThreads();
        checkForUpdatesSoon();
      } catch (e) {
        toast(`Startup: ${e}`, true);
      }
    })();
    return () => {
      unBrain();
      unRun.then((f) => f()).catch(() => {});
      unDesk.then((f) => f()).catch(() => {});
      for (const un of [unPage, unOpen, unKey]) un.then((f) => f()).catch(() => {});
      document.removeEventListener("parzi:bg", onBg);
      document.removeEventListener("parzi:layout", onLayout);
    };
  });
</script>

<svelte:window on:keydown={onKey} />

<div class="shell">
  <DefaultArt {bg} blurred={tab.kind === "page"} />

  {#if !immersive}
    {#if layout === "sidebar"}
      <Sidebar
        {tabs}
        activeTabId={activeId}
        {sessionLanes}
        on:select={(e) => selectTab(e.detail.id)}
        on:close={(e) => closeTab(e.detail.id)}
        on:move={(e) => moveTab(e.detail.id, e.detail.to)}
        on:newTab={newSession}
        on:home={goHome}
        on:search={() => (switcherOpen = true)}
        on:settings={() => openSettings()}
        on:update={() => openSettings("system")}
        on:brain={openBrain}
        on:history={() => openHistory()}
        on:setup={() => (setupOpen = true)}
      />
    {:else}
      <TopBar
    {tabs}
    activeTabId={activeId}
    {sessionLanes}
    on:select={(e) => selectTab(e.detail.id)}
    on:close={(e) => closeTab(e.detail.id)}
    on:move={(e) => moveTab(e.detail.id, e.detail.to)}
    on:newTab={newSession}
    on:home={goHome}
    on:search={() => (switcherOpen = true)}
    on:settings={() => openSettings()}
    on:update={() => openSettings("system")}
    on:brain={openBrain}
    on:history={() => openHistory()}
    on:setup={() => (setupOpen = true)}
  />
    {/if}
  {/if}

  <main>
    {#if layout === "sidebar" && !immersive}
      <!-- svelte-ignore a11y-no-static-element-interactions a11y-no-noninteractive-element-interactions -->
      <div class="winstrip" on:mousedown={startWindowDrag}>
        <span class="strip-title">{tab.title || "New session"}</span>
        <span class="winstrip-fill" />
        <WinControls />
      </div>
    {/if}
    {#if tab.kind === "settings"}
      <div class="fill col" in:fly={{ y: 8, ...motion }}>
        <PanelHeader title="Settings" on:close={() => closeTab(activeId)} />
        <Settings bind:section={settingsSection} on:close={() => closeTab(activeId)} />
      </div>
    {:else if tab.kind === "brain"}
      <div class="fill col" in:fade={{ duration: 150 }}>
        <PanelHeader title="Brain" on:close={() => closeTab(activeId)} />
        <BrainView />
      </div>
    {:else if tab.kind === "history"}
      <div class="fill col" in:fade={{ duration: 150 }}>
        <PanelHeader title="History" on:close={() => closeTab(activeId)} />
        <HistoryView
          {threads}
          {running}
          bind:view={historyView}
          on:open={(e) => openPageNext(e.detail.url)}
          on:openSession={(e) => openSession(e.detail.id)}
          on:deleteSession={(e) => remove(e.detail.id)}
          on:clearSessions={clearFinishedSessions}
        />
      </div>
    {:else if tab.kind === "preview"}
      <div class="fill col" in:fade={{ duration: 150 }}>
        <PanelHeader title={tab.title} on:close={() => closeTab(activeId)} />
        <PreviewView which={tab.url ?? "omnibar"} />
      </div>
    {:else if tab.kind === "page"}
      {#key tab.id}
        <PageView
          bind:this={page}
          {tab}
          {immersive}
          on:navigate={(e) => navigatePage(e.detail.url)}
          on:error={(e) => toast(e.detail.text, true)}
        />
      {/key}
    {:else}
      {#if hasSession}
        <section class="session" style:--dock="{dockHeight}px">
          <SessionHeader
            title={meta?.title || tab.title}
            {branch}
            lane={meta?.lane ?? ""}
            agentCount={familyActive}
            {panelOpen}
            backTitle={backTitle}
            canAct={!!shown}
            contextUsed={context.used}
            contextLimit={context.limit}
            {compacting}
            {streaming}
            tokensIn={meta?.tokens_in ?? 0}
            tokensOut={meta?.tokens_out ?? 0}
            costUsd={meta?.cost_usd ?? 0}
            on:rename={(e) => rename(e.detail.title)}
            on:fork={fork}
            on:copyId={copyId}
            on:delete={() => shown && remove(shown)}
            on:back={goBack}
            on:compact={() => onCommand("compact")}
            on:agents={() => {
              panelTab = "agents";
              panelOpen = !panelOpen;
            }}
          />
          <div class="workarea">
            <div class="scroll" bind:this={scrollEl} on:scroll={() => (farFromBottom = !nearBottom())}>
              <Thread
                {events}
                liveText={live}
                {liveReasoning}
                {liveTools}
                {approval}
                question={openQuestion}
                {streaming}
                {folder}
                on:voted={(e) => (approvals = approvals.filter((a) => a.key !== e.detail.key))}
                on:answered={(e) => (questions = questions.filter((q) => q.key !== e.detail.key))}
                on:edited={(e) => {
                  tab.composer.input = e.detail.text;
                  tabs = tabs;
                  omnibar?.focus();
                  toast("Loaded into composer — edit and send");
                }}
                on:forked={() => fork()}
              />
            </div>
            {#if panelOpen}
              <SidePanel
                {threads}
                {running}
                projectSlug={project?.slug ?? ""}
                bind:dockW={panelW}
                sessionId={shown ?? ""}
                bind:tab={panelTab}
                on:openSession={(e) => openSession(e.detail.id)}
                on:stopSession={(e) => stopSession(e.detail.id)}
                on:settled={() => void refreshThreads()}
                on:close={() => (panelOpen = false)}
              />
            {/if}
          </div>
        </section>
      {/if}

      {#if !hasSession}
        <div class="home" in:fade={{ duration: 200 }}>
          <HomeView
            {threads}
            {running}
            on:open={(e) => browse(e.detail.url)}
            on:openSession={(e) => openSession(e.detail.id)}
            on:allSessions={() => openHistory("sessions")}
          />
        </div>
      {/if}

      <div
        class="composer"
        class:docked={hasSession}
        class:shifted={hasSession && panelOpen}
        style:--shift="{panelOpen && hasSession ? panelW / 2 : 0}px"
        bind:clientHeight={dockHeight}
      >
        {#if hasSession && meta?.lane && normLane(meta.lane) !== mode && mode !== "search"}
          <div class="lane-hint">↵ starts a new {MODE_META[mode].label} session</div>
        {/if}
        <Omnibar
          bind:this={omnibar}
          bind:input={tab.composer.input}
          bind:model={tab.composer.model}
          bind:effort={tab.composer.effort}
          bind:permission
          bind:mode={tab.composer.mode}
          bind:attachments={tab.composer.attachments}
          {folder}
          lockMode={lockMode}
          folderLocked={!!tab.sessionId}
          {branch}
          {streaming}
          board={$board}
          hero={!hasSession}
          {project}
          variant={6}
          on:send={send}
          on:browse={(e) => browse(e.detail.url)}
          on:stop={stop}
          on:command={(e) => onCommand(e.detail.name)}
          on:unavailable={() => openSettings("providers")}
          on:folder={(e) => patchTab(tab.id, { cwd: e.detail.path })}
          on:project={() => loadProject(folder)}
          on:brain={openBrain}
        />
      </div>
    {/if}
  </main>

  <Switcher
    open={switcherOpen}
    {threads}
    {tabs}
    activeTabId={activeId}
    on:close={() => {
      switcherOpen = false;
      omnibar?.focus();
    }}
    on:selectTab={(e) => selectTab(e.detail.id)}
    on:openSession={(e) => openSession(e.detail.id)}
    on:deleteSession={(e) => remove(e.detail.id)}
    on:newSession={newSession}
    on:newPage={() => addTab(pageTab())}
    on:settings={() => openSettings()}
    on:previews={() => openPreview()}
    on:setup={() => (setupOpen = true)}
    on:brain={openBrain}
    on:history={() => openHistory()}
  />

  {#if setupOpen}
    <Onboarding on:close={() => (setupOpen = false)} on:openBrain={openBrain} />
  {/if}

  <div class="toasts" aria-live="polite">
    {#each $toasts as t (t.id)}
      {#if t.session}
        <button
          class="toast go"
          transition:fade={{ duration: 140 }}
          on:click={() => {
            toasts.update((all) => all.filter((x) => x.id !== t.id));
            if (immersive) void setImmersive(false);
            openSession(t.session ?? "");
          }}>{t.text}</button
        >
      {:else}
        <div class="toast" class:err={t.err} transition:fade={{ duration: 140 }}>{t.text}</div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .shell {
    position: relative;
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 10px;
    background: var(--bg);
  }
  :global(html.parzi-maximized) .shell {
    border-radius: 0;
  }
  :global(html.parzi-sidebar) .shell {
    flex-direction: row;
  }
  main {
    position: relative;
    z-index: 1;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .winstrip {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    flex: none;
    padding: 0 6px 0 14px;
    user-select: none;
  }
  .strip-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--muted);
  }
  .winstrip-fill {
    flex: 1;
  }
  .fill {
    flex: 1;
    min-height: 0;
    display: flex;
    margin: 8px 8px 0;
    overflow: hidden;
    background: var(--glass-bg-solid);
    border: var(--glass-border);
    border-bottom: none;
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
  }
  .fill.col {
    flex-direction: column;
  }
  .session {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 8px 8px 0;
    overflow: hidden;
    background: var(--glass-bg-solid);
    border: var(--glass-border);
    border-bottom: none;
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
    animation: enter 600ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  @keyframes enter {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .workarea {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .scroll {
    flex: 1;    overflow-x: hidden;
    overflow-y: auto;
    padding-bottom: calc(var(--dock) + 30px);
    mask-image: linear-gradient(
      to bottom,
      #000 calc(100% - var(--dock) - 28px),
      transparent calc(100% - var(--dock) - 4px)
    );
  }
  .home {
    position: absolute;
    top: calc(50% + 4px);
    left: 50%;
    z-index: 10;
    width: min(720px, 90%);
    max-height: calc(50% - 24px);
    overflow-y: auto;
    transform: translateX(-50%);
  }
  .composer {
    position: absolute;
    left: 50%;
    bottom: calc(54% - 58px);
    z-index: 20;
    width: min(720px, 90%);
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
    transition:
      bottom 1050ms cubic-bezier(0.22, 1, 0.36, 1),
      transform 1050ms cubic-bezier(0.22, 1, 0.36, 1),
      width 700ms ease;
  }
  .composer.docked {
    bottom: 14px;
    width: min(740px, calc(100% - 40px));
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
  }
  .lane-hint {
    margin: 0 0 6px;
    text-align: center;
    font-size: 11.5px;
    color: var(--faint);
  }
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 1200;
    display: flex;
    flex-direction: column;
    gap: 6px;
    pointer-events: none;
  }
  .toast {
    max-width: 360px;
    padding: 8px 14px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    text-align: left;
  }
  .toast.go {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
    cursor: pointer;
    pointer-events: auto;
  }
  .toast.go:hover {
    background: color-mix(in srgb, var(--text) 6%, var(--panel));
  }
  .toast.err {
    border-color: var(--bad);
    color: var(--bad);
  }
</style>
