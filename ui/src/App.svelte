<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api, brain, deskSync, onBrowser, onBrowserKey, onBrowserOpen, onDesk, onRunEvent,
    type AgentRequest, type ChatEvent, type ComposerMode, type PageEvent, type PlanQuote, type Question, type SessionMeta, type UiEvent,
  } from "./lib/api";
  import { downloads as downloadApi, onBrowserAsk, onDownload, type Download } from "./lib/api";
  import { addPrompt, downloadList, dropTabPrompts, upsertDownload } from "./lib/browserState";
  import { closedTabs, recordFavicon } from "./lib/browserData";
  import { downloadShare, popClosed, pushClosed } from "./lib/pageRules";
  import TopBar from "./lib/TopBar.svelte";
  import Switcher from "./lib/Switcher.svelte";
  import DefaultArt from "./lib/DefaultArt.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import PanelHeader from "./lib/PanelHeader.svelte";
  import SessionInfo from "./lib/SessionInfo.svelte";
  import PlanInline from "./lib/PlanInline.svelte";
  import Thread from "./lib/Thread.svelte";
  import SessionHeader from "./lib/SessionHeader.svelte";
  import PageView from "./lib/PageView.svelte";
  import HistoryView from "./lib/HistoryView.svelte";
  import PreviewView from "./lib/PreviewView.svelte";
  import BrainView from "./lib/BrainView.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import { loadTabs, onboarded, recordVisit, saveTabs, titleVisit } from "./lib/browserData";
  import Settings from "./lib/Settings.svelte";
  import Icon from "./lib/Icon.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { isRemote, loadRemoteInfo, onRemoteStatus, refreshRemoteBoard, remote, remoteBoard, remoteInfo } from "./lib/remote";
  import { normLane, permissionFor } from "./lib/lanes";
  import { provideClearSessions } from "./lib/sessions";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce } from "./lib/threadList";
  import { checkForUpdatesSoon } from "./lib/updateStore";
  import { brainTab, ensureComposer, historyTab, hostOf, isExplicitUrl, pageTab, previewTab, rememberModel, sessionTab, settingsTab, toAddress, type Tab, type TabComposerState } from "./lib/tabs";
  import { openBrainNote, brainTabRequested } from "./lib/brainStore";
  import { sessionMarks, watchActivity, type SessionMark } from "./lib/activity";
  import { addQuote, MAX_QUOTES, quoteBlock, sameSlug, splitQuotes } from "./lib/planModel";
  import { toast, toastError, toasts, notify, chime, dismissToast, keyedToast } from "./lib/toast";
  import { nameOf } from "./lib/providerRows";
  import { addNotification, dropNotification, markSessionRead, type NotificationItem } from "./lib/notificationStore";
  import { covered } from "./lib/overlay";
  import { tooltip } from "./lib/tooltip";
  import type { Approval, LiveTool } from "./lib/live";
  import { deskRows, type DeskCmd } from "./lib/desk";
  import { watchPlan } from "./lib/planStore";

  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motion = reduced ? { duration: 0 } : { duration: 200, easing: cubicOut };

  // Home is the one draft: a session tab with no session yet. It never
  // shows in the strip; the Home button opens it, and sending from it turns
  // it into a real tab at the strip's end (a new Home comes when needed).
  const isHome = (t: Tab) => t.kind === "session" && !t.sessionId;

  // Restored drafts fold into one Home, kept last, out of the strip's way.
  function oneHome(list: Tab[]): Tab[] {
    const home = list.find(isHome);
    return [...list.filter((t) => !isHome(t)), ...(home ? [home] : [])];
  }

  const restored = loadTabs();
  let tabs: Tab[] = oneHome(
    (restored?.tabs ?? [sessionTab()]).map((t) => {
      const c = ensureComposer((t as Partial<Tab>).composer);
      if (c.model === "auto") c.model = "";
      return { ...t, composer: c };
    }),
  );
  let activeId = restored?.active ?? tabs[0].id;
  let threads: SessionMeta[] = [];
  let bg = "";

  let shown: string | null = null;
  let meta: SessionMeta | null = null;
  let events: ChatEvent[] = [];
  let running = new Set<string>();
  let approvals: Approval[] = [];
  let questions: Question[] = [];
  let requests: AgentRequest[] = [];
  // Waiting on you beats working: that is the one to look at.
  $: sessionMarks.set(
    Object.fromEntries<SessionMark>([
      ...[...running].map((id) => [id, "running"] as const),
      ...[...approvals, ...questions, ...requests].map((x) => [x.session, "waiting"] as const),
    ]),
  );
  let context = { used: 0, limit: 0 };
  let compacting = false;
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTools: LiveTool[] = [];
  let pending = "";
  let flushTimer = 0;

  let model = "";
  let warmed = "";
  // Sent as `mode` with every agent message. Starts from Settings'
  // default (lanes.default_mode, loaded on mount); never full by default.
  let permission = permissionFor("");
  // Settings' default_mode: the floor the composer's choice cannot lift.
  let policy = "";
  let mode: ComposerMode = "build";
  let sending = false;
  // Type-ahead. The backend takes one run per session, so a message sent
  // while its session works is held here (a later one joins it) and goes
  // out when the run is done.
  type Held = { tab: string; text: string; quotes: PlanQuote[]; files: string[]; model: string; effort: string; mode: string; cwd: string; lane: string; remote: boolean };
  let held: Record<string, Held> = {};
  $: heldNow = tab.kind === "session" && shown ? held[shown] : undefined;
  $: heldText = heldNow ? heldNow.text || heldNow.quotes.map((q) => q.title).join(", ") : "";

  // The Omnibar binds to `comp`, the active tab's own composer object
  // (tab.composer, same reference), so each tab keeps its draft, mode,
  // model, effort and attachments. `comp` is only ever assigned inside
  // functions, never by a `$:` statement: a binding to a reactive value
  // invalidates everything that value derives from, which put every
  // keystroke through `tabs` (desk sync, tab save, switcher rebuild).
  let comp: TabComposerState = tabs.find((t) => t.id === activeId)?.composer ?? tabs[0].composer;
  let compFor = "";

  function syncComposer(t: Tab) {
    t.composer = ensureComposer(t.composer);
    if (compFor === t.id && comp === t.composer) return;
    compFor = t.id;
    comp = t.composer;
  }

  function laneOf(sessionId: string | null | undefined): string {
    if (!sessionId) return "";
    return threads.find((t) => t.id === sessionId)?.lane ?? "";
  }

  // Legacy sessions stored lane "code" (today's "build") or "research"
  // (today's "work"): normalized in lib/lanes.ts.
  // The nearest session up the parent chain that has a tab: a helper's
  // tab sits under it in the strip.
  function hostSessionOf(id: string): string | null {
    let cur = threads.find((t) => t.id === id)?.parent_id ?? null;
    for (let guard = 0; cur && guard < 50; guard++) {
      if (tabs.some((t) => t.kind === "session" && t.sessionId === cur)) return cur;
      cur = threads.find((t) => t.id === cur)?.parent_id ?? null;
    }
    return null;
  }

  // One parent map per threads change instead of a find() per hop.
  $: familyActive = (() => {
    if (!shown) return 0;
    const parent = new Map(threads.map((t) => [t.id, t.parent_id]));
    let n = 0;
    for (const t of threads) {
      if (t.id === shown || !(t.status === "active" || t.status === "queued" || running.has(t.id))) continue;
      let cur = t.parent_id;
      let guard = 0;
      while (cur && guard++ < 50) {
        if (cur === shown) {
          n++;
          break;
        }
        cur = parent.get(cur);
      }
    }
    return n;
  })();

  // Composer state lives on the tab: switching tabs preserves each
  // tab's composer intact, with no copying and no cross-contamination.

  let switcherOpen = false;
  // The column beside a session: open unless you closed it, and only where
  // the window has room for it.
  let infoOpen = true;
  try {
    infoOpen = localStorage.getItem("parzi.info.open") !== "0";
  } catch {}
  let innerWidth = 1280;
  // The plan checklist for a session that works on no item, asked for from
  // the column; it resets when another session shows.
  let planForced = false;
  let planForcedFor: string | null = null;
  let planInline = false;
  let settingsSection = "general";
  let scrollEl: HTMLElement | null = null;
  let farFromBottom = false;
  let dockHeight = 90;
  let omnibar: Omnibar;
  let page: PageView;
  let immersive = false;
  let setupOpen = false;
  let setupStep = 0;
  let setupServer = false;
  // The last Remote switch position; new drafts start from it.
  let preferRemote = false;
  try {
    // Linked means the server is home: Remote is on until switched off.
    preferRemote = localStorage.getItem("parzi.remote.prefer") !== "0";
  } catch {}

  async function setRemote(on: boolean) {
    comp.remote = on;
    comp = comp;
    preferRemote = on;
    try {
      localStorage.setItem("parzi.remote.prefer", on ? "1" : "0");
    } catch {}
    if (!on) return;
    try {
      // Opens the link, updating Parzi on the server when it is older.
      remoteInfo.set(await remote.connect());
      await refreshRemoteBoard();
    } catch (e) {
      toast(`Can't reach the server, so this stays on this PC: ${e}`, true);
      comp.remote = false;
      comp = comp;
      preferRemote = false;
      try {
        localStorage.setItem("parzi.remote.prefer", "0");
      } catch {}
    }
  }
  let project: { slug: string; title: string; tokens: number } | null = null;
  let navSeq = 0;
  let deskRev = 0;

  $: tab = tabs.find((t) => t.id === activeId) ?? tabs[0];
  $: syncComposer(tab);
  // Only mode/model mirror out (lane hint, agent warmup).
  $: mode = comp.mode;
  $: model = comp.model;
  $: streaming = !!shown && running.has(shown);
  $: hasSession = tab.kind === "session" && (!!shown || sending || events.length > 0);
  // Where this composer sends: a started session stays where it began; a
  // draft follows its own switch, else the last choice.
  $: remoteOn = tab.kind === "session" && tab.sessionId ? isRemote(tab.sessionId) : !!$remoteInfo && (comp.remote ?? preferRemote);
  // A server session's folder is on the server: nothing to list or branch here.
  $: folder = remoteOn ? "" : tab.kind === "session" && tab.sessionId ? (meta?.cwd ?? "") : (tab.cwd ?? "");
  $: approval = approvals.find((a) => a.session === shown) ?? null;
  $: openQuestion = questions.find((q) => q.session === shown) ?? null;
  $: openRequest = requests.find((r) => r.session === shown) ?? null;
  // Requests wait on you as questions do, in the plan and the session column.
  $: asks = [...questions, ...requests.map((r) => ({ key: r.key, session: r.session, question: r.request, options: [] }))];
  $: sessionLanes = Object.fromEntries(threads.map((t) => [t.id, t.lane ?? ""]));
  // Folder-driven side effects fire on value change only: without the
  // guard every keystroke (same folder string, fresh dirty flag) would
  // re-invoke git_branch + brain_context and pile backend work behind typing.
  let lastFolder = "@@none@@";
  $: if (folder !== lastFolder) {
    lastFolder = folder;
    void loadBranch(folder);
    void loadProject(folder);
  }
  // Page events (loading, back/forward, blocked counts) patch tabs all
  // the time; the desk and the saved tab list only care about this.
  $: tabsKey = JSON.stringify([activeId, tabs.map((t) => [t.id, t.kind, t.title, t.url ?? "", t.sessionId ?? "", t.owner ?? "", t.group ?? "", t.cwd ?? ""])]);
  let lastTabsKey = "";
  $: if (tabsKey !== lastTabsKey) {
    lastTabsKey = tabsKey;
    syncDesk(tabs, activeId);
    saveTabs(tabs, activeId);
  }
  $: pageVisible = tab.kind === "page";
  $: toastsUp = pageVisible && !immersive;
  const LAST_PAGE = "parzi.lastPage";
  let lastPageVisible: boolean | null = null;
  $: if (pageVisible !== lastPageVisible) {
    lastPageVisible = pageVisible;
    if (!pageVisible) api.browserHide().catch(() => {});
    if (!pageVisible && immersive) void setImmersive(false);
  }

  const refreshThreads = coalesce(async () => {
    threads = await api.listThreads().catch(() => threads);
  }, 100);

  function patchTab(id: string, patch: Partial<Tab>) {
    tabs = tabs.map((t) => (t.id === id ? { ...t, ...patch } : t));
  }

  function syncDesk(list: Tab[], active: string) {
    deskSync(deskRows(list), deskRev, active).catch(() => {});
  }

  async function loadProject(dir: string) {
    const ctx = dir ? await brain.context(dir).catch(() => null) : null;
    // A quick tab switch can land an older folder's answer last.
    if (dir !== folder) return;
    project = ctx?.project ? { slug: ctx.project.slug, title: ctx.project.title, tokens: ctx.tokens } : null;
  }

  let historyView: "sessions" | "history" | "bookmarks" | "downloads" = "sessions";

  function openHistory(view = historyView) {
    historyView = view;
    const existing = tabs.find((t) => t.kind === "history");
    if (existing) selectTab(existing.id);
    else addTab(historyTab());
  }

  function reopenClosed() {
    const [last, rest] = popClosed($closedTabs);
    if (!last) return;
    closedTabs.set(rest);
    openPageNext(last.url);
  }

  function openBrain(path: string | unknown = "") {
    if (typeof path === "string" && path) openBrainNote(path);
    const existing = tabs.find((t) => t.kind === "brain");
    if (existing) selectTab(existing.id);
    else addTab(brainTab());
  }

  // The session's project: its plan and its name.
  $: planProject = (hasSession ? meta?.project : "") || project?.slug || "";
  $: planTitle = project && sameSlug(project.slug, planProject) ? project.title : "";
  $: if (hasSession) watchPlan(planProject, folder);
  $: infoShown = hasSession && infoOpen && innerWidth >= 900;
  $: if (shown !== planForcedFor) {
    planForcedFor = shown;
    planForced = false;
  }

  function toggleInfo(open = !infoShown) {
    infoOpen = open;
    try {
      localStorage.setItem("parzi.info.open", open ? "1" : "0");
    } catch {}
    if (open && hasSession && innerWidth < 900) toast("Widen the window to see the session column");
  }

  // Agents ask for "the plan": the checklist shows in the thread.
  function showPlan() {
    planForced = true;
    if (!hasSession) toast("Open a session in a project to see its plan");
    else void tick().then(() => scrollEl?.querySelector(".plan")?.scrollIntoView({ block: "nearest", behavior: reduced ? "auto" : "smooth" }));
  }

  // Clicking a plan item quotes it into this tab's composer.
  function quoteItem(q: PlanQuote) {
    const list = comp.quotes ?? [];
    const next = addQuote(list, q);
    if (next === list && list.length >= MAX_QUOTES) toast(`A message quotes at most ${MAX_QUOTES} plan items`);
    comp.quotes = next;
    if (comp.mode === "search") comp.mode = "build";
    comp = comp;
    omnibar?.focus();
  }

  function useExample(text: string) {
    comp.input = text;
    if (comp.mode === "search") comp.mode = "build";
    comp = comp;
    omnibar?.focus();
  }

  // An agent renamed its session: the list, the tab and the header follow.
  async function renamed(id: string) {
    await refreshThreads();
    const t = threads.find((x) => x.id === id);
    if (!t?.title) return;
    tabs = tabs.map((x) => (x.sessionId === id && x.title !== t.title ? { ...x, title: t.title } : x));
    if (meta?.id === id) meta = { ...meta, title: t.title };
  }

  // Set up Parzi, opened at its end with the server form showing.
  function openRemoteSetup() {
    setupStep = 3;
    setupServer = true;
    setupOpen = true;
  }

  function openSetup() {
    setupStep = 0;
    setupServer = false;
    setupOpen = true;
  }

  async function loadBranch(dir: string) {
    const name = dir ? await api.gitBranch(dir).catch(() => "") : "";
    if (dir === folder) branch = name;
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
    markSessionRead(id);
    clearLive();
    try {
      const [m, ev] = await api.getThread(id);
      if (my !== navSeq) return;
      meta = m;
      events = ev;
      context = { used: m.context_tokens ?? 0, limit: m.context_limit ?? 0 };
      // A session on an agent's own default ("cursor") keeps it too.
      if (m.model) {
        tab.composer.model = m.model;
        comp = comp;
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
    let got: [SessionMeta, ChatEvent[]];
    try {
      got = await api.getThread(id);
    } catch (e) {
      if (my === navSeq && shown === id) toastError(e);
      return;
    }
    const [m, ev] = got;
    if (my !== navSeq || shown !== id) return;
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
    if (next.kind === "session") {
      if (next.sessionId) void showSession(next.sessionId);
      else showDraft();
    }
    // The page warmed at the next launch: the one used last.
    if (next.kind === "page") {
      try {
        localStorage.setItem(LAST_PAGE, id);
      } catch {}
    }
  }

  function addTab(t: Tab) {
    t.composer = ensureComposer(t.composer);
    tabs = [...tabs, t];
    selectTab(t.id);
  }

  // `to` is a place in the strip, which leaves Home out.
  function moveTab(id: string, to: number) {
    const strip = tabs.filter((t) => !isHome(t));
    const from = strip.findIndex((t) => t.id === id);
    if (from < 0 || from === to) return;
    const [moved] = strip.splice(from, 1);
    strip.splice(Math.max(0, Math.min(to, strip.length)), 0, moved);
    tabs = [...strip, ...tabs.filter(isHome)];
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
        dropTabPrompts(dead);
        const url = t.url;
        if (url) closedTabs.update((list) => pushClosed(list, { url, title: t.title, at: Date.now() }));
      }
    }
    if (settingsReturn && doomed.has(settingsReturn)) settingsReturn = null;
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

  // Home's project cards set the draft's folder, as the composer's picker does.
  function useFolder(path: string) {
    if (remoteOn) {
      comp.remoteCwd = path;
      comp = comp;
    } else {
      patchTab(tab.id, { cwd: path });
    }
    omnibar?.focus();
  }

  // A new session starts from Home: it becomes a tab when you send.
  function goHome() {
    let home = tabs.find(isHome);
    if (!home) {
      home = { ...sessionTab(), cwd: folder || undefined };
      home.composer = ensureComposer(home.composer);
      tabs = [...tabs, home];
    }
    selectTab(home.id);
    void tick().then(() => omnibar?.focus());
  }

  const newSession = goHome;

  function openPage(url: string, id?: string, owner?: string | null, background = false) {
    const existing = tabs.find((t) => (id ? t.id === id : t.kind === "page" && t.url === url));
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

  // A link in a session's thread opens under that session, after the pages
  // already there, and shows. The page stays the person's own.
  function openInSession(url: string, sessionId: string) {
    const existing = tabs.find((t) => t.kind === "page" && t.url === url);
    if (existing) {
      if (!existing.owner && !existing.group) patchTab(existing.id, { group: sessionId });
      selectTab(existing.id);
      return;
    }
    const t = pageTab(url, undefined, null, sessionId);
    const mine = tabs.map((x, i) => (x.sessionId === sessionId || x.owner === sessionId || x.group === sessionId ? i : -1)).filter((i) => i >= 0);
    const at = mine.length ? Math.max(...mine) + 1 : tabs.findIndex((x) => x.id === activeId) + 1;
    tabs = [...tabs.slice(0, at), t, ...tabs.slice(at)];
    selectTab(t.id);
  }

  function openPageNext(url: string) {
    const t = pageTab(url);
    const idx = tabs.findIndex((x) => x.id === activeId);
    tabs = [...tabs.slice(0, idx + 1), t, ...tabs.slice(idx + 1)];
    selectTab(t.id);
  }

  // A link that opens a new window: your page's opens in front, next to it;
  // an agent's page keeps its owner and opens behind, like the agent's own.
  function openFromPage(opener: string, url: string) {
    const from = tabs.find((t) => t.id === opener);
    if (from?.group) {
      openInSession(url, from.group);
      return;
    }
    if (!from?.owner) {
      openPageNext(url);
      return;
    }
    const t = pageTab(url, undefined, from.owner);
    const idx = tabs.findIndex((x) => x.id === opener);
    tabs = [...tabs.slice(0, idx + 1), t, ...tabs.slice(idx + 1)];
    api.browserPrepare(t.id, url, from.owner).catch(() => {});
  }

  async function recoverPage(id: string) {
    try {
      await api.browserRecover(id);
    } catch (e) {
      toastError(e);
    }
    // The old page has to be gone before the next show makes a new one.
    await new Promise((r) => setTimeout(r, 250));
    patchTab(id, { crashed: undefined, loading: false });
  }

  function onDownloadEvent(d: Download) {
    upsertDownload(d);
    const key = `download-${d.id}`;
    if (d.state === "progress") {
      keyedToast(key, `Downloading ${d.name}`, { progress: downloadShare(d), ms: 0 });
    } else if (d.state === "done") {
      keyedToast(key, `Downloaded ${d.name}`, {
        ms: 8000,
        action: { label: "Show in folder", run: () => void downloadApi.reveal(d.id).catch(toastError) },
      });
    } else if (d.state === "failed") {
      keyedToast(key, `Download failed: ${d.name}`, { err: true, ms: 6000 });
    } else {
      keyedToast(key, null);
    }
  }

  function navigatePage(url: string) {
    const id = tab.id;
    patchTab(id, { url, title: hostOf(url) });
    api.browserNavigate(id, url).catch(toastError);
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
    if (e.crashed) patch.crashed = e.crashed;
    if (e.favicon) {
      patch.favicon = e.favicon;
      const at = e.url ?? t.url;
      if (at) recordFavicon(at, e.favicon);
    }
    // A new load starts without the last site's icon or questions.
    if (e.loading === true && t.favicon) patch.favicon = undefined;
    if (e.loading === true) dropTabPrompts(e.tab);
    if (e.popupBlocked) addPrompt({ id: `popup-${e.tab}-${Date.now()}`, tab: e.tab, kind: "popup", url: e.popupBlocked });
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
        // Failed loads (offline, no such host) stay out of history and
        // address completion.
        if (e.ok !== false) recordVisit(e.url, pending ?? "");
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
    const existing = tabs.find((t) => t.sessionId === id);
    if (existing) {
      selectTab(existing.id);
      return;
    }
    const title = threads.find((t) => t.id === id)?.title || "Session";
    // A helper (an agent's, or a side task) opens under the session it came
    // from, after what is already there: the strip doesn't grow.
    const host = hostSessionOf(id);
    if (host) {
      const t = { ...sessionTab(id, title), group: host };
      t.composer = ensureComposer(t.composer);
      const mine = tabs.map((x, i) => (x.sessionId === host || x.group === host || x.owner === host ? i : -1)).filter((i) => i >= 0);
      const at = Math.max(...mine) + 1;
      tabs = [...tabs.slice(0, at), t, ...tabs.slice(at)];
      selectTab(t.id);
      return;
    }
    if (tab.kind === "session" && !tab.sessionId && !events.length) {
      patchTab(tab.id, { sessionId: id, title });
      selectTab(tab.id);
    } else {
      addTab(sessionTab(id, title));
    }
  }


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
    const draft = target.composer;
    const prompt = draft.input.trim();
    // A message can be just quoted plan items.
    const quotes = draft.mode === "search" ? [] : [...(draft.quotes ?? [])];
    if ((!prompt && !quotes.length) || sending) return;
    if (!draft.model && draft.mode !== "search") {
      toast("Pick a model first");
      omnibar?.openModels();
      return;
    }
    if (!quotes.length && isExplicitUrl(prompt)) {
      draft.input = "";
      comp = comp;
      browse(toAddress(prompt));
      return;
    }
    const files = [...draft.attachments];
    // A thread's conversation lives inside one agent: another agent's
    // model starts a new session instead of failing the send.
    const agentOf = (model: string) => model.split("/")[0];
    const sessionModel = threads.find((t) => t.id === target.sessionId)?.model ?? "";
    const otherAgent = !!sessionModel && !!draft.model && agentOf(sessionModel) !== agentOf(draft.model);
    const fresh = !target.sessionId || otherAgent;
    // A session keeps its lane. A new one builds when it has a folder to
    // work in, and is a plain assistant (no shell) without one.
    const where = remoteOn ? (target.sessionId ? (meta?.cwd ?? "") : (draft.remoteCwd ?? "")) : folder;
    const lane = !fresh ? normLane(laneOf(target.sessionId)) || "build" : where ? "build" : "work";
    if (!fresh && target.sessionId && running.has(target.sessionId)) {
      hold(target.sessionId, { tab: target.id, text: prompt, quotes, files, model: draft.model, effort: draft.effort, mode: permission, cwd: where, lane, remote: remoteOn });
      draft.input = "";
      draft.attachments = [];
      draft.quotes = [];
      comp = comp;
      return;
    }
    if (target.sessionId && otherAgent) toast(`New session on ${nameOf(agentOf(draft.model))}: a thread stays with the agent it started on`);
    // Quotes go as blocks after the text, as the desk will send them.
    const optimistic: ChatEvent = { kind: "user", text: [prompt, ...quotes.map(quoteBlock)].filter(Boolean).join("\n\n") };
    sending = true;
    draft.input = "";
    draft.attachments = [];
    draft.quotes = [];
    comp = comp;
    events = [...events, optimistic];
    try {
      const sid = await api.sendMessage({
        sessionId: fresh ? null : target.sessionId,
        model: draft.model,
        prompt,
        // A started server session keeps its folder there; a draft uses the
        // server folder picked for it.
        cwd: where,
        effort: draft.effort,
        attachments: files,
        mode: permission,
        lane,
        remote: remoteOn,
        quotes: quotes.map((q) => ({ project: q.project, id: q.id })),
      });
      running = new Set(running).add(sid);
      rememberModel(draft.model, draft.effort);
      // The field was cleared before the await. Drop the stored draft
      // for the tab that sent, not whichever tab is active now.
      draft.input = "";
      draft.attachments = [];
      draft.quotes = [];
      comp = comp;
      if (fresh) patchTab(target.id, { sessionId: sid, title: (prompt || quotes[0]?.title || "New session").slice(0, 40) });
      // Another device's session continues on the server under a new id.
      else if (sid !== target.sessionId) patchTab(target.id, { sessionId: sid });
      if (activeId === target.id) {
        shown = sid;
        await reload();
        await scrollToBottom();
      }
      void refreshThreads();
    } catch (e) {
      draft.input = prompt;
      draft.attachments = files;
      draft.quotes = quotes;
      comp = comp;
      events = events.filter((ev) => ev !== optimistic);
      toastError(e);
    } finally {
      sending = false;
    }
  }

  function hold(sid: string, next: Held) {
    const prev = held[sid];
    held = {
      ...held,
      [sid]: prev
        ? {
            ...next,
            text: [prev.text, next.text].filter(Boolean).join("\n\n"),
            quotes: [...prev.quotes, ...next.quotes].slice(0, MAX_QUOTES),
            files: [...new Set([...prev.files, ...next.files])],
          }
        : next,
    };
  }

  function take(sid: string): Held | undefined {
    const h = held[sid];
    if (!h) return undefined;
    const rest = { ...held };
    delete rest[sid];
    held = rest;
    return h;
  }

  // A held message goes back into its composer, ahead of anything typed
  // since: its run stopped short, or sending it failed.
  function restore(sid: string, h: Held) {
    const t = tabs.find((x) => x.id === h.tab) ?? tabs.find((x) => x.sessionId === sid);
    if (!t) return;
    t.composer = ensureComposer(t.composer);
    const c = t.composer;
    c.input = [h.text, c.input.trim()].filter(Boolean).join("\n\n");
    c.attachments = [...new Set([...h.files, ...c.attachments])];
    c.quotes = [...h.quotes, ...(c.quotes ?? [])].slice(0, MAX_QUOTES);
    if (t.id === activeId) {
      syncComposer(t);
      comp = comp;
    }
  }

  function unhold(sid: string) {
    const h = take(sid);
    if (h) restore(sid, h);
  }

  // The run a held message waited on is done: it goes out now.
  async function sendHeld(sid: string) {
    if (!held[sid]) return;
    // Busy from here, so a message typed meanwhile is held with it.
    running = new Set(running).add(sid);
    if (sid === shown) await reload();
    const h = take(sid);
    if (!h) return;
    const optimistic: ChatEvent = { kind: "user", text: [h.text, ...h.quotes.map(quoteBlock)].filter(Boolean).join("\n\n") };
    if (sid === shown) {
      events = [...events, optimistic];
      void scrollToBottom();
    }
    try {
      const id = await api.sendMessage({
        sessionId: sid,
        model: h.model,
        prompt: h.text,
        cwd: h.cwd,
        effort: h.effort,
        attachments: h.files,
        mode: h.mode,
        lane: h.lane,
        remote: h.remote,
        quotes: h.quotes.map((q) => ({ project: q.project, id: q.id })),
      });
      if (id !== sid) {
        running.delete(sid);
        running = new Set(running).add(id);
        for (const t of tabs.filter((t) => t.sessionId === sid)) patchTab(t.id, { sessionId: id });
      }
      if (shown === sid) {
        shown = id;
        await reload();
        await scrollToBottom();
      }
      void refreshThreads();
    } catch (e) {
      running.delete(sid);
      running = running;
      if (shown === sid) events = events.filter((ev) => ev !== optimistic);
      // Anything typed while it was going out stays after it.
      unhold(sid);
      restore(sid, h);
      toastError(e);
    }
  }

  async function stop() {
    if (!shown || !running.has(shown)) return;
    const id = shown;
    running.delete(id);
    running = running;
    unhold(id);
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
            () => reject(new Error("Compact timed out: the summarizer is taking too long. You can try again.")),
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

  // A side task: a new session started now, beside this one, from what the
  // person typed (or a message waiting on the run). It lists under this
  // session and never writes to it. "@project" points it at that project.
  async function sideTask(fromHeld = false) {
    const parent = shown;
    if (!parent || tab.kind !== "session" || !tab.sessionId) return;
    let text = "";
    let quotes: PlanQuote[] = [];
    let files: string[] = [];
    if (fromHeld) {
      const h = take(parent);
      if (!h) return;
      ({ text, quotes, files } = h);
    } else {
      text = comp.input.trim();
      quotes = [...(comp.quotes ?? [])];
      files = [...comp.attachments];
      if (!text && !quotes.length) return;
      comp.input = "";
      comp.attachments = [];
      comp.quotes = [];
      comp = comp;
    }
    if (!comp.model) {
      toast("Pick a model first");
      omnibar?.openModels();
      comp.input = text;
      comp = comp;
      return;
    }
    const aimed = await aimAt(text);
    const where = aimed.folder || (remoteOn ? (meta?.cwd ?? "") : folder);
    const prompt = [aimed.text, sideBrief(meta?.title || tab.title, events, aimed.title)].filter(Boolean).join("\n\n");
    try {
      const id = await api.sendMessage({
        sessionId: null,
        model: comp.model,
        prompt,
        cwd: where,
        effort: comp.effort,
        attachments: files,
        mode: permission,
        lane: where ? "build" : "work",
        remote: remoteOn,
        quotes: quotes.map((q) => ({ project: q.project, id: q.id })),
        parent,
      });
      running = new Set(running).add(id);
      void refreshThreads();
      const busySame = !aimed.folder && running.has(parent);
      toast(busySame ? "Side task started in this folder too: the two may edit the same files" : `Side task started${aimed.title ? ` in ${aimed.title}` : ""}`);
    } catch (e) {
      comp.input = [text, comp.input].filter(Boolean).join("\n\n");
      comp.attachments = [...files, ...comp.attachments];
      comp = comp;
      toastError(e);
    }
  }

  // "@website ..." aims a side task at that project's folder.
  async function aimAt(text: string): Promise<{ text: string; folder: string; title: string }> {
    const m = /(^|\s)@([\w.-]+)/.exec(text);
    if (!m) return { text, folder: "", title: "" };
    const want = m[2].toLowerCase();
    const list = await brain.projects().catch(() => []);
    const p = list.find((x) => x.folder && (x.slug.toLowerCase() === want || x.title.toLowerCase().replace(/\s+/g, "-") === want));
    if (!p) return { text, folder: "", title: "" };
    return { text: text.replace(m[0], m[1]).trim(), folder: p.folder, title: p.title };
  }

  // What a side task knows of the session it came from: what that one was
  // asked and last said, so "do the same for the website" makes sense.
  function sideBrief(title: string, evs: ChatEvent[], elsewhere: string): string {
    const clip = (s: string, n: number) => (s.length > n ? `${s.slice(0, n).trimEnd()}…` : s);
    const last = (kind: "user" | "assistant") => {
      const e = [...evs].reverse().find((x) => x.kind === kind && x.text.trim());
      return e && "text" in e ? splitQuotes(e.text).text.replace(/\n?\[attached: [^\]]+\]/, "").trim() : "";
    };
    const asked = last("user");
    const said = last("assistant");
    // Tagged so the thread shows it as a small label, not as your words.
    const from = title.replace(/["<>]/g, "");
    return [
      `<side-task from="${from}">`,
      `This is a side task, started from the session "${from}", which keeps working on its own. Leave that session's work alone${elsewhere ? `, and work in the project ${elsewhere}` : ""}.`,
      asked ? `That session was asked: ${clip(asked, 600)}` : "",
      said ? `It last said: ${clip(said, 600)}` : "",
      "</side-task>",
    ]
      .filter(Boolean)
      .join("\n");
  }

  // A failed turn's "Try again": the last message goes out once more.
  function retryLast() {
    const last = [...events].reverse().find((e) => e.kind === "user");
    if (!last || last.kind !== "user" || streaming || sending) return;
    comp.input = last.text
      .replace(/\n?\[attached: [^\]]+\]/, "")
      .replace(/\n*<side-task[^>]*>[\s\S]*?<\/side-task>\n*/, "")
      .trimEnd();
    comp = comp;
    void send();
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
      take(id);
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
    navigator.clipboard.writeText(shown).then(
      () => toast("Session ID copied"),
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
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

  const titleOf = (session: string) => threads.find((t) => t.id === session)?.title || "Agent";

  // One announcement per event: a toast and a bell entry, both skipped for
  // the session you are looking at. Approvals and questions also chime,
  // since the backend denies them after a while.
  function announce(session: string, what: string, kind: NotificationItem["kind"], opts: { key?: string; chime?: boolean } = {}) {
    if (session === shown && tab.kind === "session" && document.hasFocus()) return;
    const title = titleOf(session);
    notify(`${title} ${what}`, session);
    addNotification({ title, body: what[0].toUpperCase() + what.slice(1), kind, sessionId: session, key: opts.key });
    if (opts.chime !== undefined) chime(opts.chime);
  }

  function settle(key: string) {
    approvals = approvals.filter((a) => a.key !== key);
    questions = questions.filter((q) => q.key !== key);
    requests = requests.filter((r) => r.key !== key);
    dropNotification(key);
  }

  function onEvent(e: UiEvent) {
    if (e.kind === "approval") {
      const fresh = !approvals.some((a) => a.key === e.key);
      approvals = [...approvals.filter((a) => a.key !== e.key), { key: e.key, session: e.session, call: e.call }];
      if (fresh) announce(e.session, `needs your approval: ${e.call?.label || e.call?.name || "an action"}`, "approval", { key: e.key, chime: false });
      return;
    }
    if (e.kind === "question") {
      const fresh = !questions.some((q) => q.key === e.key);
      questions = [...questions.filter((q) => q.key !== e.key), { key: e.key, session: e.session, question: e.question, options: e.options }];
      if (fresh) announce(e.session, `asks: ${e.question || "a question"}`, "question", { key: e.key, chime: false });
      return;
    }
    if (e.kind === "request") {
      const fresh = !requests.some((r) => r.key === e.key);
      requests = [...requests.filter((r) => r.key !== e.key), { key: e.key, session: e.session, request: e.request, kind: e.req_kind }];
      if (fresh) announce(e.session, `needs something from you: ${e.request || "a request"}`, "question", { key: e.key, chime: false });
      return;
    }
    if (e.kind === "expired") {
      settle(e.key);
      return;
    }
    if (e.kind === "done" || e.kind === "error" || e.kind === "paused") {
      running.delete(e.session);
      running = running;
      for (const k of [...approvals, ...questions, ...requests].filter((x) => x.session === e.session).map((x) => x.key)) settle(k);
      void refreshThreads();
      // A held message goes out after a finished run, else back to its composer.
      const next = e.kind === "done" && !!held[e.session];
      if (e.kind !== "done") unhold(e.session);
      if (e.session === shown) {
        flushLive();
        clearLive();
        if (!next) void reload();
      }
      if (next) void sendHeld(e.session);
      const isChild = threads.some((t) => t.id === e.session && t.parent_id);
      if (e.kind === "paused") announce(e.session, "paused at its spending limit", "error", { chime: false });
      else if (e.kind === "done") announce(e.session, "finished", isChild ? "subagent" : "success", { chime: true });
      else announce(e.session, "stopped with an error", "error", { chime: false });
      return;
    }
    if (!running.has(e.session) && e.kind !== "context") {
      running = new Set(running).add(e.session);
    }
    if (e.kind === "tool_result" && e.name === "session.rename" && e.ok) void renamed(e.session);
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
    } else if (e.kind === "notice") {
      toast(e.text);
    }
  }

  function shortcut(name: string): boolean {
    if (name === "ctrl+p" || name === "ctrl+k") switcherOpen = !switcherOpen;
    else if (name === "ctrl+t") newSession();
    else if (name === "ctrl+w") {
      if (!isHome(tab)) closeTab(activeId);
    }
    else if (name === "ctrl+comma") tab.kind === "settings" ? closeTab(activeId) : openSettings();
    else if (name === "ctrl+b") openBrain();
    else if (name === "ctrl+j" || name === "ctrl+shift+a") toggleInfo();
    else if (name === "ctrl+h") openHistory();
    else if (name === "ctrl+shift+t") reopenClosed();
    else if (name === "f11") void setImmersive(!immersive);
    else if (name === "ctrl+l") {
      if (tab.kind === "page") {
        void setImmersive(false);
        void page?.focusAddress();
      } else if (tab.kind === "session") {
        comp.mode = "search";
        omnibar?.focus();
      }
    } else if (name === "ctrl+tab" || name === "ctrl+shift+tab") {
      // Cycles the strip; from Home it starts at either end.
      const strip = tabs.filter((t) => !isHome(t));
      if (strip.length) {
        const idx = strip.findIndex((t) => t.id === activeId);
        const step = name === "ctrl+tab" ? 1 : -1;
        selectTab(strip[idx < 0 ? (step > 0 ? 0 : strip.length - 1) : (idx + step + strip.length) % strip.length].id);
      }
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
              : mod && ["p", "k", "t", "w", "l", "b", "h", "j"].includes(key)
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

  // Commands from the control socket (the CLI and agents). A page opens
  // behind the current tab unless the command asked for focus; the CLI
  // asks by default, agents do not.
  function onDeskCmd(cmd: DeskCmd) {
    if (cmd.op === "open" && cmd.url) openPage(cmd.url, cmd.id, cmd.owner ?? null, !cmd.focus);
    else if (cmd.op === "navigate" && cmd.id && cmd.url) patchTab(cmd.id, { url: cmd.url, title: hostOf(cmd.url) });
    else if (cmd.op === "focus" && cmd.id) selectTab(cmd.id);
    else if (cmd.op === "close" && cmd.id) closeTab(cmd.id);
    else if (cmd.op === "show") showFromDesk(cmd);
    else if (cmd.op === "notify" && cmd.text) {
      notify(cmd.text, cmd.session ?? "");
      chime(true);
    } else if (cmd.op === "approval" && cmd.key) {
      // Answered from the command line: the card goes, and says why.
      approvals = approvals.filter((a) => a.key !== cmd.key);
      if (cmd.label) toast(`${cmd.allow ? "Allowed" : "Denied"} from the command line: ${cmd.label}`);
    } else if (cmd.op === "deleted" && cmd.id) {
      const gone = cmd.id;
      running.delete(gone);
      running = running;
      for (const t of tabs.filter((x) => x.sessionId === gone)) closeTab(t.id);
      void refreshThreads();
    }
  }

  function showFromDesk(cmd: DeskCmd) {
    if (cmd.what === "session" && cmd.id) {
      const id = cmd.id;
      if (cmd.focus !== false) openSession(id);
      else if (!tabs.some((t) => t.sessionId === id)) {
        const t = sessionTab(id, cmd.title || "Session");
        const idx = tabs.findIndex((x) => x.id === activeId);
        tabs = [...tabs.slice(0, idx + 1), t, ...tabs.slice(idx + 1)];
      }
    } else if (cmd.what === "settings") openSettings(cmd.section || "general");
    else if (cmd.what === "brain") openBrain(cmd.path ?? "");
    else if (cmd.what === "history") openHistory(cmd.view ?? "sessions");
    else if (cmd.what === "plan" || cmd.what === "work" || cmd.what === "tasks") showPlan();
    else if (cmd.what === "home") goHome();
  }

  // Only the page used last loads ahead at launch: every renderer costs
  // memory, and the other tabs load when opened.
  async function warmPages() {
    await new Promise((r) => setTimeout(r, 1500));
    let last = "";
    try {
      last = localStorage.getItem(LAST_PAGE) ?? "";
    } catch {}
    const t = tabs.find((x) => x.id === last && x.kind === "page" && x.url && x.id !== activeId);
    if (t) await api.browserPrepare(t.id, t.url ?? "", t.owner ?? null).catch(() => {});
  }

  onMount(() => {
    const unRun = onRunEvent(onEvent);
    const unDesk = onDesk((cmd) => {
      if (typeof cmd.rev === "number") deskRev = Math.max(deskRev, cmd.rev);
      onDeskCmd(cmd);
    });
    const unPage = onBrowser(onPage);
    const unOpen = onBrowserOpen((e) => openFromPage(e.tab, e.url));
    const unKey = onBrowserKey((name) => void shortcut(name));
    const unAsk = onBrowserAsk((a) => addPrompt({ ...a }));
    const unDownload = onDownload(onDownloadEvent);
    downloadApi
      .list()
      .then((list) => downloadList.set(list))
      .catch(() => {});
    let brainSeen = 0;
    const unActivity = watchActivity();
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
    // Settings › General changed the tool policy: the composer follows.
    const onDefaultMode = (e: Event) => {
      policy = (e as CustomEvent<string>).detail ?? "";
      permission = permissionFor(policy);
    };
    document.addEventListener("parzi:default-mode", onDefaultMode);
    // A chat link to a local page (an agent's preview, a dev server) opens
    // as a tab, reusing the agent's page when one shows the same URL.
    const onOpenPage = (e: Event) => {
      const url = (e as CustomEvent<string>).detail ?? "";
      if (!url) return;
      if (tab.kind === "session" && tab.sessionId) openInSession(url, tab.sessionId);
      else openPage(url);
    };
    document.addEventListener("parzi:open-page", onOpenPage);
    const unClear = provideClearSessions(clearFinishedSessions);
    api
      .getConfig()
      .then((cfg) => {
        policy = cfg.lanes?.default_mode ?? "";
        permission = permissionFor(policy);
      })
      .catch((e) => toast(`Couldn't read the tool policy, so agents will ask first: ${e}`, true));
    api
      .listThreads()
      .then((list) => (threads = list))
      .catch((e) => toast(`Couldn't load sessions: ${e}`, true));
    void loadRemoteInfo().catch(() => {});
    // The link to the server: updates, and a stream that restarted.
    const unRemote = onRemoteStatus((s) => {
      if (s.state === "updating" || s.state === "ready") toast(s.detail);
      else if (s.state === "error") toast(s.detail, true);
      if (s.state === "up" || s.state === "resync" || s.state === "ready") {
        void refreshThreads();
        if (remoteOn) void refreshRemoteBoard().catch(() => {});
        if (isRemote(shown)) void reload();
      }
      // Another device's sessions and the shared settings arrive by sync.
      if (s.state === "synced") void refreshThreads();
      if (s.state === "settings") {
        void api
          .getConfig()
          .then((cfg) => {
            policy = cfg.lanes?.default_mode ?? "";
            permission = permissionFor(policy);
          })
          .catch(() => {});
      }
    });
    (async () => {
      try {
        applyThemeCss(await api.getThemeCss());
        bg = await api.backgroundUrl();
        await ensureBoard();
        checkForUpdatesSoon();
      } catch (e) {
        toast(`Startup: ${e}`, true);
      }
    })();
    return () => {
      unBrain();
      unActivity();
      unClear();
      document.removeEventListener("parzi:default-mode", onDefaultMode);
      document.removeEventListener("parzi:open-page", onOpenPage);
      unRun.then((f) => f()).catch(() => {});
      unDesk.then((f) => f()).catch(() => {});
      unRemote.then((f) => f()).catch(() => {});
      for (const un of [unPage, unOpen, unKey, unAsk, unDownload]) un.then((f) => f()).catch(() => {});
      document.removeEventListener("parzi:bg", onBg);
    };
  });
</script>

<svelte:window on:keydown={onKey} bind:innerWidth />

<div class="shell">
  <DefaultArt {bg} blurred={tab.kind === "page"} />

  {#if !immersive}
    <TopBar
      tabs={tabs.filter((t) => !isHome(t))}
      homeActive={isHome(tab)}
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
      on:setup={openSetup}
      on:openSession={(e) => openSession(e.detail.id)}
    />
  {/if}

  <div class="content">
    <main>
    {#if tab.kind === "settings"}
      <div class="fill col" in:fly={{ y: 8, ...motion }}>
        <PanelHeader title="Settings" on:close={() => closeTab(activeId)} />
        <Settings bind:section={settingsSection} on:close={() => closeTab(activeId)} on:setupRemote={openRemoteSetup} />
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
          on:openTab={(e) => openPageNext(e.detail.url)}
          on:recover={() => recoverPage(tab.id)}
          on:resume={() => patchTab(tab.id, { crashed: undefined })}
        />
      {/key}
    {:else}
      {#if hasSession}
        <div class="stagerow">
          <section class="session">
            <SessionHeader
              title={meta?.title || tab.title}
              {branch}
              lane={meta?.lane ?? ""}
              {folder}
              agentCount={familyActive}
              panelOpen={infoShown}
              canAct={!!shown}
              contextUsed={context.used}
              contextLimit={infoShown ? 0 : context.limit}
              {compacting}
              {streaming}
              costUsd={meta?.cost_usd ?? 0}
              selfCompacts={meta && !["claude", "opencode"].includes(meta.model.split("/")[0]) ? nameOf(meta.model.split("/")[0]) : ""}
              on:rename={(e) => rename(e.detail.title)}
              on:fork={fork}
              on:copyId={copyId}
              on:delete={() => shown && remove(shown)}
              on:compact={() => onCommand("compact")}
              on:plan={() => toggleInfo()}
            />
            <div class="workarea">
              <div class="scroll" style:--dock="{dockHeight}px" bind:this={scrollEl} on:scroll={() => (farFromBottom = !nearBottom())}>
                <Thread
                  {events}
                  liveText={live}
                  {liveReasoning}
                  {liveTools}
                  {approval}
                  question={openQuestion}
                  request={openRequest}
                  askerTitle={threads.find((t) => t.id === (openQuestion?.session ?? openRequest?.session ?? approval?.session ?? shown))?.title ?? ""}
                  {streaming}
                  {folder}
                  provider={threads.find((t) => t.id === shown)?.model.split("/")[0] ?? ""}
                  sessionTitle={titleOf}
                  on:voted={(e) => settle(e.detail.key)}
                  on:answered={(e) => settle(e.detail.key)}
                  on:edited={(e) => {
                    comp.input = e.detail.text;
                    if (e.detail.quotes.length) comp.quotes = e.detail.quotes.slice(0, MAX_QUOTES);
                    comp = comp;
                    omnibar?.focus();
                    toast("Loaded into composer. Edit and send.");
                  }}
                  on:forked={() => fork()}
                  on:retry={retryLast}
                  on:compact={compact}
                >
                  <PlanInline
                    project={planProject}
                    session={shown ?? ""}
                    {threads}
                    {running}
                    {approvals}
                    questions={asks}
                    force={planForced}
                    bind:visible={planInline}
                    on:quote={(e) => quoteItem(e.detail)}
                  />
                </Thread>
              </div>
            </div>
          </section>
          {#if infoShown}
            <SessionInfo
              session={shown ?? ""}
              project={planProject}
              projectTitle={planTitle}
              {folder}
              {branch}
              lane={meta?.lane ?? ""}
              remoteHost={remoteOn ? ($remoteInfo?.host ?? "") : ""}
              contextUsed={context.used}
              contextLimit={context.limit}
              {compacting}
              {streaming}
              selfCompacts={meta && !["claude", "opencode"].includes(meta.model.split("/")[0]) ? nameOf(meta.model.split("/")[0]) : ""}
              {threads}
              {running}
              {approvals}
              questions={asks}
              {planInline}
              on:openSession={(e) => openSession(e.detail.id)}
              on:openUrl={(e) => openPage(e.detail.url)}
              on:compact={compact}
              on:plan={showPlan}
            />
          {/if}
        </div>
      {/if}

      <div
        class="composer"
        class:docked={hasSession}
        style:--shift="{infoShown ? 136 : 0}px"
        bind:clientHeight={dockHeight}
      >
        <Omnibar
          bind:this={omnibar}
          bind:input={comp.input}
          bind:model={comp.model}
          bind:effort={comp.effort}
          bind:permission
          {policy}
          bind:mode={comp.mode}
          bind:attachments={comp.attachments}
          bind:quotes={comp.quotes}
          {folder}
          folderLocked={!!tab.sessionId}
          {branch}
          {streaming}
          board={remoteOn ? $remoteBoard : $board}
          remoteHost={$remoteInfo?.host ?? ""}
          remote={remoteOn}
          remoteLocked={!!tab.sessionId}
          on:remote={(e) => setRemote(e.detail.on)}
          remoteCwd={comp.remoteCwd ?? ""}
          on:remoteFolder={(e) => {
            comp.remoteCwd = e.detail.path;
            comp = comp;
          }}
          hero={!hasSession}
          {project}
          held={heldText}
          on:send={send}
          on:unhold={() => shown && unhold(shown)}
          on:side={() => void sideTask(false)}
          on:sideHeld={() => void sideTask(true)}
          canSide={tab.kind === "session" && !!tab.sessionId}
          on:browse={(e) => browse(e.detail.url)}
          on:stop={stop}
          on:command={(e) => onCommand(e.detail.name)}
          on:unavailable={() => openSettings(remoteOn ? "connections" : "providers")}
          on:folder={(e) => patchTab(tab.id, { cwd: e.detail.path })}
          on:project={() => loadProject(folder)}
          on:brain={openBrain}
        />
      </div>
    {/if}
    </main>
  </div>

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
    on:setup={openSetup}
    on:brain={openBrain}
    on:history={() => openHistory()}
  />

  {#if setupOpen}
    <Onboarding startStep={setupStep} server={setupServer} on:close={() => (setupOpen = false)} on:openBrain={openBrain} />
  {/if}

  <!-- While a web page is shown its native view covers the bottom corner:
       toasts move up into the window chrome, newest only. -->
  <div class="toasts" class:chrome={toastsUp} aria-live="polite">
    {#each toastsUp ? $toasts.slice(-1) : $toasts as t (t.id)}
      {#if t.run}
        <button
          class="toast go"
          class:err={t.err}
          transition:fade={{ duration: 140 }}
          on:click={() => {
            dismissToast(t.id);
            t.run?.();
          }}>{t.text}</button
        >
      {:else if t.session}
        <button
          class="toast go"
          transition:fade={{ duration: 140 }}
          on:click={() => {
            dismissToast(t.id);
            if (immersive) void setImmersive(false);
            openSession(t.session ?? "");
          }}>{t.text}</button
        >
      {:else}
        <div class="toast" class:err={t.err} class:acts={!!t.action || t.progress !== undefined} transition:fade={{ duration: 140 }}>
          <span class="toast-text">{t.text}</span>
          {#if t.progress !== undefined}
            <span class="toast-bar" aria-hidden="true"><span style:width="{Math.round(t.progress * 100)}%" /></span>
          {/if}
          {#if t.action}
            {@const act = t.action}
            <button
              class="toast-act"
              on:click={() => {
                dismissToast(t.id);
                act.run();
              }}>{act.label}</button
            >
          {/if}
        </div>
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
    border-radius: var(--radius);
    background: var(--bg);
  }
  :global(html.parzi-maximized) .shell {
    border-radius: 0;
  }
  /* The shell is always a column: the top bar spans the full width and
     the sidebar (when enabled) sits inside .content next to main. (An
     older row direction here crumpled the top bar into a dead column
     once the top bar became always-on.) */
  main {
    position: relative;
    z-index: 1;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .content {
    display: flex;
    flex: 1;
    min-height: 0;
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
  /* The session and, beside it, the cards about it. */
  .stagerow {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .session {
    position: relative;
    min-width: 0;
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
    animation: enter 600ms var(--ease-out);
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
    flex: 1;
    overflow-x: hidden;
    overflow-y: auto;
    padding-bottom: calc(var(--dock) + 30px);
    mask-image: linear-gradient(
      to bottom,
      #000 calc(100% - var(--dock) - 28px),
      transparent calc(100% - var(--dock) - 4px)
    );
  }
  /* Home is the composer alone, a little above the middle. In a session
     it docks at the bottom of the thread, beside the info column. */
  .composer {
    position: absolute;
    left: 50%;
    bottom: 46%;
    z-index: 20;
    width: min(680px, calc(90% - 2 * var(--shift, 0px)));
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
    transition: width 300ms ease;
  }
  .composer.docked {
    bottom: 14px;
    /* With the dock open it fits the thread beside it, not the window. */
    width: min(740px, calc(100% - 40px - 2 * var(--shift, 0px)));
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
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
    font-size: var(--fs-sm);
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
  .toast.acts {
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: auto;
  }
  .toast-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .toast-bar {
    display: block;
    flex: none;
    width: 64px;
    height: 4px;
    margin-left: 8px;
    border-radius: 999px;
    background: var(--line);
    overflow: hidden;
  }
  .toast-bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 200ms var(--ease-out);
  }
  .toast-act {
    flex: none;
    min-height: 24px;
    padding: 2px 10px;
    background: var(--hover);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .toast-act:hover {
    background: var(--selected);
  }
  /* Over a page: one line in the top bar, left of the window buttons. */
  .toasts.chrome {
    top: 5px;
    right: 150px;
    bottom: auto;
  }
  .toasts.chrome .toast {
    display: flex;
    align-items: center;
    max-width: min(440px, 45vw);
    min-height: 28px;
    padding: 2px 6px 2px 12px;
    white-space: nowrap;
  }
  .toasts.chrome .toast:not(.acts) {
    padding-right: 12px;
  }
</style>
