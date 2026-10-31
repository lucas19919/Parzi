<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api, brain, deskSync, onBrowser, onBrowserKey, onBrowserOpen, onDesk, onResync, onRunEvent,
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
  import ImageViewer from "./lib/ImageViewer.svelte";
  import PlanInline from "./lib/PlanInline.svelte";
  import Thread from "./lib/Thread.svelte";
  import PageView from "./lib/PageView.svelte";
  import ManagerView from "./lib/ManagerView.svelte";
  import PreviewView from "./lib/PreviewView.svelte";
  import ArtifactCard from "./lib/widgets/ArtifactCard.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import { loadTabs, onboarded, recordVisit, saveTabs, titleVisit } from "./lib/browserData";
  import Settings from "./lib/Settings.svelte";
  import Icon from "./lib/Icon.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { isRemote, loadRemoteInfo, onRemoteStatus, refreshRemoteBoard, remote, remoteBoard, remoteInfo } from "./lib/remote";
  import { permissionFor } from "./lib/permission";
  import { provideClearSessions } from "./lib/sessions";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce } from "./lib/threadList";
  import { checkForUpdatesSoon } from "./lib/updateStore";
  import { artifactTab, ensureComposer, hostOf, isExplicitUrl, pageTab, previewTab, rememberModel, sessionTab, toAddress, type ArtifactDoc, type Tab, type TabComposerState } from "./lib/tabs";

  import { sessionMarks, watchActivity, type SessionMark } from "./lib/activity";
  import { addQuote, MAX_QUOTES, quoteBlock, sameSlug, splitQuotes } from "./lib/planModel";
  import { toast, toastError, toasts, notify, chime, dismissToast, keyedToast } from "./lib/toast";
  import { nameOf } from "./lib/providerRows";
  import { follower } from "./lib/follow";
  import { draftKey, flushDrafts, loadDrafts, sameDraft, saveDrafts, withDraft } from "./lib/drafts";
import { dropId, enqueue, flushHeld, heldLabel, loadHeld, MAX_HELD, moveHeld, moveId, putFront, saveHeld, stillWorking, takeAll, takeHead, takeId, type HeldItem, type HoldQueue } from "./lib/held";
  import { addNotification, dropNotification, markSessionRead, type NotificationItem } from "./lib/notificationStore";
  import { covered } from "./lib/overlay";
  import { popover, placeAbove } from "./lib/popover";
  import { saveFile } from "./lib/saveFile";
  import { tooltip } from "./lib/tooltip";
  import type { Approval, LiveTool } from "./lib/live";
  import { deskRows, type DeskCmd } from "./lib/desk";
  import { watchPlan } from "./lib/planStore";

  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motion = reduced ? { duration: 0 } : { duration: 200, easing: cubicOut };

  const isDraft = (t: Tab) => t.kind === "session" && !t.sessionId;
  const emptyDraft = (t: Tab) => isDraft(t) && !t.composer?.input?.trim() && !t.composer?.attachments?.length;

  function oneEmptyDraft(list: Tab[]): Tab[] {
    let kept = false;
    return list.filter((t) => {
      if (!emptyDraft(t)) return true;
      if (kept) return false;
      kept = true;
      return true;
    });
  }

  const restored = loadTabs();
  let tabs: Tab[] = oneEmptyDraft(
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
  let liveTodo: unknown = null;
  let liveTools: LiveTool[] = [];
  let pending = "";
  let revealing = 0;

  let model = "";
  let warmed = "";
  let permission = permissionFor("");
  let policy = "";
  let mode: ComposerMode = "build";
  let sending = false;
  let held: HoldQueue = loadHeld();
  $: saveHeld(held);
  let queuePaused = new Set<string>();
  $: if ([...queuePaused].some((sid) => !(held[sid]?.length))) {
    queuePaused = new Set([...queuePaused].filter((sid) => held[sid]?.length));
  }
  $: heldRows = (tab.kind === "session" && shown ? held[shown] ?? [] : []).map((item) => ({
    id: item.id,
    text: heldLabel(item),
    detail: [item.files.length ? `${item.files.length} file${item.files.length === 1 ? "" : "s"}` : "", item.quotes.length ? `${item.quotes.length} quote${item.quotes.length === 1 ? "" : "s"}` : ""].filter(Boolean).join(" · "),
  }));
  $: queueIsPaused = !!shown && queuePaused.has(shown);
  const inflight = new Set<string>();
  const holdLive = new Map<string, number>();
  const released = new Map<string, number>();

  function markLive(id: string) {
    released.delete(id);
    holdLive.set(id, Date.now() + 2000);
    running = new Set(running).add(id);
  }

  function release(id: string) {
    holdLive.delete(id);
    released.set(id, Date.now() + 2000);
    if (!running.has(id)) return;
    running.delete(id);
    running = new Set(running);
  }

  function persistDrafts() {
    flushDrafts();
    flushHeld();
  }

  let comp: TabComposerState = tabs.find((t) => t.id === activeId)?.composer ?? tabs[0].composer;
  let compFor = "";
  let liveModel = "";
  $: if (comp.model !== liveModel) liveModel = comp.model;

  function syncComposer(t: Tab) {
    t.composer = ensureComposer(t.composer);
    if (compFor === t.id && comp === t.composer) return;
    compFor = t.id;
    fillDraft(t);
    comp = t.composer;
  }

  let drafts = loadDrafts();

  function fillDraft(t: Tab) {
    const c = t.composer;
    if (t.kind !== "session" || c.input.trim() || c.attachments.length || c.quotes?.length) return;
    const d = drafts[draftKey(t)];
    if (!d) return;
    c.input = d.input;
    c.quotes = [...d.quotes].slice(0, MAX_QUOTES);
    c.attachments = [...d.attachments];
  }

  function keepDraft(t: Tab | undefined, d: { input: string; quotes: PlanQuote[]; attachments: string[] }) {
    if (t?.kind !== "session") return;
    const key = draftKey(t);
    if (sameDraft(drafts[key], d)) return;
    drafts = withDraft(drafts, key, d);
    saveDrafts(drafts);
  }

  function forgetDraft(t: Tab) {
    keepDraft(t, { input: "", quotes: [], attachments: [] });
  }

  $: keepDraft(
    tabs.find((t) => t.id === compFor && t.composer === comp),
    { input: comp.input, quotes: comp.quotes ?? [], attachments: comp.attachments },
  );

  function hostSessionOf(id: string): string | null {
    let cur = threads.find((t) => t.id === id)?.parent_id ?? null;
    for (let guard = 0; cur && guard < 50; guard++) {
      if (tabs.some((t) => t.kind === "session" && t.sessionId === cur)) return cur;
      cur = threads.find((t) => t.id === cur)?.parent_id ?? null;
    }
    return null;
  }

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


  let switcherOpen = false;
  let infoOpen = true;
  try {
    infoOpen = localStorage.getItem("parzi.info.open") !== "0";
  } catch {}
  let infoWidth = 264;
  try {
    const w = parseInt(localStorage.getItem("parzi.info.width") ?? "", 10);
    if (w >= 220 && w <= 520) infoWidth = w;
  } catch {}
  let innerWidth = 1280;
  let planForced: Set<string> = new Set();
  $: planForcedNow = !!shown && planForced.has(shown);
  let planInline = false;
  let settingsSection = "general";
  let scrollEl: HTMLElement | null = null;
  let dockHeight = 90;
  let omnibar: Omnibar;
  let page: PageView;
  let immersive = false;
  let setupOpen = false;
  let setupStep = 0;
  let setupServer = false;
  let preferRemote = false;
  try {
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
      remoteInfo.set(await remote.connect());
      await refreshRemoteBoard();
    } catch (e) {
      toast(`Can't reach the remote, so this stays on this PC: ${e}`, true);
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
  $: mode = comp.mode;
  $: model = comp.model;
  $: streaming = !!shown && running.has(shown);
  $: hasSession = tab.kind === "session" && (!!shown || sending || events.length > 0);
  $: remoteOn = tab.kind === "session" && tab.sessionId ? isRemote(tab.sessionId) : !!$remoteInfo && (comp.remote ?? preferRemote);
  $: folder = remoteOn ? "" : tab.kind === "session" && tab.sessionId ? (meta?.cwd ?? "") : (tab.cwd ?? "");
  $: approval = approvals.find((a) => a.session === shown) ?? null;
  $: openQuestion = questions.find((q) => q.session === shown) ?? null;
  $: openRequest = requests.find((r) => r.session === shown) ?? null;
  $: asks = [...questions, ...requests.map((r) => ({ key: r.key, session: r.session, question: r.request, options: [] }))];
  $: parent = meta?.parent_id ? (threads.find((t) => t.id === meta?.parent_id) ?? null) : null;
  let lastFolder = "@@none@@";
  $: if (folder !== lastFolder) {
    lastFolder = folder;
    void loadBranch(folder);
    void loadProject(folder);
  }
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
    noteThreads(threads);
  }, 100);

  function applyLive(id: string, status: string) {
    const live = status === "active" || status === "queued";
    if (live) {
      if ((released.get(id) ?? 0) > Date.now()) return;
      if (!running.has(id)) running = new Set(running).add(id);
      return;
    }
    if (inflight.has(id) || !running.has(id)) return;
    if ((holdLive.get(id) ?? 0) > Date.now()) return;
    running.delete(id);
    running = new Set(running);
  }

  function noteThreads(list: { id: string; status: string }[]) {
    for (const t of list) applyLive(t.id, t.status);
  }

  function patchTab(id: string, patch: Partial<Tab>) {
    tabs = tabs.map((t) => (t.id === id ? { ...t, ...patch } : t));
  }

  function syncDesk(list: Tab[], active: string) {
    deskSync(deskRows(list), deskRev, active).catch(() => {});
  }

  async function loadProject(dir: string) {
    const ctx = dir ? await brain.context(dir).catch(() => null) : null;
    if (dir !== folder) return;
    project = ctx?.project ? { slug: ctx.project.slug, title: ctx.project.title, tokens: ctx.tokens } : null;
  }

  let cover: "settings" | "doc" | "sessions" | null = null;
  let coverFor = "";
  $: coverOn = cover !== null && tab.kind === "session" && activeId === coverFor;
  $: showWallpaper = tab.kind === "session" && !hasSession && !coverOn;

  function clearCover() {
    cover = null;
    coverFor = "";
    docMenuOpen = false;
  }

  let docMenuOpen = false;
  let docMenuBtn: HTMLButtonElement | null = null;
  let docMenuStyle = "";
  let docCopied = false;

  function toggleDocMenu() {
    docMenuOpen = !docMenuOpen;
    if (docMenuOpen && docMenuBtn) docMenuStyle = placeAbove(docMenuBtn, 220);
  }

  function copyPinnedDoc() {
    docMenuOpen = false;
    if (!pinnedDoc) return;
    navigator.clipboard.writeText(pinnedDoc.content).then(
      () => {
        docCopied = true;
        setTimeout(() => (docCopied = false), 1200);
      },
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
  }

  function savePinnedDoc() {
    docMenuOpen = false;
    if (!pinnedDoc) return;
    void saveFile(`${pinnedDoc.title.replace(/\.[a-z0-9]{1,8}$/i, "") || "note"}.md`, pinnedDoc.content);
  }

  function openPinnedDocFolder() {
    docMenuOpen = false;
    if (!pinnedDoc) return;
    void brain.open(pinnedDoc.id, "folder").catch(toastError);
  }

  function sessionHost(): string {
    if (tab.kind === "session") return tab.id;
    const home = tabs.find((t) => t.kind === "session" && !t.sessionId);
    if (home) return home.id;
    const any = tabs.find((t) => t.kind === "session");
    if (any) return any.id;
    const draft = sessionTab();
    draft.composer = ensureComposer(draft.composer);
    tabs = [...tabs, draft];
    return draft.id;
  }

  function showCover(kind: "settings" | "doc" | "sessions") {
    switcherOpen = false;
    const id = sessionHost();
    cover = kind;
    coverFor = id;
    if (activeId !== id) selectTab(id);
  }

  function reopenClosed() {
    const [last, rest] = popClosed($closedTabs);
    if (!last) return;
    closedTabs.set(rest);
    openPageNext(last.url);
  }

  $: planProject = (hasSession ? meta?.project : "") || project?.slug || "";
  $: planTitle = project && sameSlug(project.slug, planProject) ? project.title : "";
  $: if (tab.kind === "session") watchPlan(planProject, folder);
  $: infoShown = tab.kind === "session" && infoOpen && innerWidth >= 900;

  $: shift = Math.round((infoShown ? infoWidth + 8 : 0) / 2);

  function toggleInfo(open = !infoShown) {
    infoOpen = open;
    try {
      localStorage.setItem("parzi.info.open", open ? "1" : "0");
    } catch {}
    if (open && hasSession && innerWidth < 900) toast("Widen the window to see the session column");
  }

  function resizeInfo(e: { detail: { width: number } }) {
    infoWidth = Math.min(520, Math.max(220, Math.round(e.detail.width)));
    try {
      localStorage.setItem("parzi.info.width", String(infoWidth));
    } catch {}
  }

  function showPlan() {
    if (!hasSession) {
      if (planProject) void pinNote(`projects/${planProject}/plan.md`);
      else toast("Pick a project to see its plan");
      return;
    }
    if (!shown) return;
    const next = new Set(planForced);
    if (next.has(shown)) next.delete(shown);
    else next.add(shown);
    planForced = next;
    if (next.has(shown)) void tick().then(() => scrollEl?.querySelector(".plan")?.scrollIntoView({ block: "nearest", behavior: reduced ? "auto" : "smooth" }));
  }

  function hidePlan() {
    if (!shown) return;
    const next = new Set(planForced);
    next.delete(shown);
    planForced = next;
  }

  function quoteItem(q: PlanQuote) {
    const list = comp.quotes ?? [];
    const next = addQuote(list, q);
    if (next === list && list.length >= MAX_QUOTES) toast(`A message quotes at most ${MAX_QUOTES} plan items`);
    comp.quotes = next;
    if (comp.mode === "search") comp.mode = "build";
    comp = comp;
    omnibar?.focus();
  }

  function unquoteItem(q: PlanQuote) {
    comp.quotes = (comp.quotes ?? []).filter((x) => !(x.id === q.id && sameSlug(x.project, q.project)));
    comp = comp;
  }

  function useExample(text: string) {
    comp.input = text;
    if (comp.mode === "search") comp.mode = "build";
    comp = comp;
    omnibar?.focus();
  }

  async function renamed(id: string) {
    await refreshThreads();
    const t = threads.find((x) => x.id === id);
    if (!t?.title) return;
    tabs = tabs.map((x) => (x.sessionId === id && x.title !== t.title ? { ...x, title: t.title } : x));
    if (meta?.id === id) meta = { ...meta, title: t.title };
  }

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
    cancelAnimationFrame(revealing);
    revealing = 0;
    pending = "";
    live = "";
    liveReasoning = "";
    liveTodo = null;
    liveTools = [];
  }

  const follow = follower();

  async function scrollToBottom(smooth = false) {
    await tick();
    follow.toBottom(smooth);
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
      if (m.model) {
        tab.composer.model = m.model;
        comp = comp;
      }
      applyLive(id, m.status);
      if (tab.sessionId === id && m.title) patchTab(tab.id, { title: m.title });
      await scrollToBottom();
    } catch (e) {
      if (my === navSeq) toastError(e);
    }
  }

  async function reload(swap?: () => void) {
    if (!shown) return swap?.();
    const id = shown;
    const my = navSeq;
    let got: [SessionMeta, ChatEvent[]];
    try {
      got = await api.getThread(id);
    } catch (e) {
      if (my === navSeq && shown === id) toastError(e);
      return swap?.();
    }
    const [m, ev] = got;
    if (my !== navSeq || shown !== id) return;
    meta = m;
    events = ev;
    swap?.();
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
    if (cover && id !== coverFor) clearCover();
    activeId = id;
    if (next.kind === "session") {
      if (next.sessionId) void showSession(next.sessionId);
      else showDraft();
    }
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

  function moveTab(id: string, to: number) {
    const strip = [...tabs];
    const from = strip.findIndex((t) => t.id === id);
    if (from < 0 || from === to) return;
    const [moved] = strip.splice(from, 1);
    strip.splice(Math.max(0, Math.min(to, strip.length)), 0, moved);
    tabs = strip;
  }

  function closeTab(id: string) {
    const idx = tabs.findIndex((t) => t.id === id);
    if (idx < 0) return;
    const closed = tabs[idx];
    const under = closed.kind === "page" ? (closed.owner ?? closed.group ?? null) : null;
    const doomed = new Set([id]);
    const closingSession = closed.kind === "session" ? closed.sessionId : null;
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
    const kept = tabs.filter((t) => !doomed.has(t.id));
    if (!kept.length) {
      const fresh = sessionTab();
      tabs = [fresh];
      selectTab(tabs[0].id);
      return;
    }
    tabs = kept;
    if (!doomed.has(activeId)) return;
    const home = under ? kept.find((t) => t.kind === "session" && t.sessionId === under) : undefined;
    if (home) {
      selectTab(home.id);
      return;
    }
    if (closed.kind === "artifact" && closed.group) {
      const g = closed.group as string;
      const parent = kept.find((t) => t.id === g) ?? kept.find((t) => t.kind === "session" && t.sessionId === g);
      if (parent) {
        selectTab(parent.id);
        return;
      }
    }
    selectTab(kept[Math.max(0, Math.min(idx, kept.length - 1))].id);
  }

  function useFolder(path: string) {
    if (remoteOn) {
      comp.remoteCwd = path;
      comp = comp;
    } else {
      patchTab(tab.id, { cwd: path });
    }
    omnibar?.focus();
  }

  function newSession() {
    const draft: Tab = { ...sessionTab(), cwd: folder || undefined };
    draft.composer = ensureComposer(draft.composer);
    const idx = tabs.findIndex((x) => x.id === activeId);
    tabs = [...tabs.slice(0, idx + 1), draft, ...tabs.slice(idx + 1)];
    selectTab(draft.id);
    void tick().then(() => omnibar?.focus());
  }

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

  let pinnedDocs: Record<string, ArtifactDoc> = {};
  $: pinnedDoc = shown ? (pinnedDocs[shown] ?? null) : null;

  function closePinnedDoc() {
    if (!shown) return;
    if (!(shown in pinnedDocs)) return;
    const { [shown]: _, ...rest } = pinnedDocs;
    pinnedDocs = rest;
  }

  async function pinNote(path: string) {
    const group = tab.kind === "session" ? (tab.sessionId ?? null) : (tab.group ?? null);
    if (!group) {
      toast("Open a session to pin this note beside it.");
      return;
    }
    let content: string | null = null;
    try {
      content = await brain.read(path);
    } catch {
      content = null;
    }
    if (content == null) {
      toast("That note can't be read.", true);
      return;
    }
    const prev = pinnedDocs[group];
    const version = prev && prev.id === path ? prev.version + 1 : 1;
    const title = path.split("/").pop()?.replace(/\.md$/i, "") || path;
    pinnedDocs = { ...pinnedDocs, [group]: { id: path, title, kind: "markdown", content, version } };
    if (!infoShown) toggleInfo(true);
  }

  function openArtifact(doc: ArtifactDoc, forceTab = false) {
    const group = tab.kind === "session" ? (tab.sessionId ?? null) : (tab.group ?? null);
    if (doc.kind === "markdown" && group && !forceTab) {
      pinnedDocs = { ...pinnedDocs, [group]: doc };
      if (!infoShown) toggleInfo(true);
      return;
    }
    const existing = tabs.find((t) => t.kind === "artifact" && t.doc?.id === doc.id && (t.group ?? null) === group);
    if (existing) {
      patchTab(existing.id, { doc, title: doc.title });
      selectTab(existing.id);
      return;
    }
    const t = artifactTab(doc, group);
    const mine = group ? tabs.map((x, i) => (x.sessionId === group || x.owner === group || x.group === group ? i : -1)).filter((i) => i >= 0) : [];
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
    if (e.zoom !== undefined) patch.zoom = Math.abs(e.zoom - 1) < 0.005 ? undefined : e.zoom;
    if (e.bg) patch.bg = e.bg;
    if (e.crashed) patch.crashed = e.crashed;
    if (e.favicon) {
      patch.favicon = e.favicon;
      const at = e.url ?? t.url;
      if (at) recordFavicon(at, e.favicon);
    }
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

  function openSettings(section = "general") {
    settingsSection = section === "system" || section === "remote" || section === "skills" ? "general" : section;
    showCover("settings");
  }

  async function send() {
    const target = tab;
    const draft = target.composer;
    const prompt = draft.input.trim();
    const quotes = draft.mode === "search" ? [] : [...(draft.quotes ?? [])];
    if (!prompt && !quotes.length) return;
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
    const agentOf = (model: string) => model.split("/")[0];
    const sessionModel = threads.find((t) => t.id === target.sessionId)?.model ?? "";
    const otherAgent = !!sessionModel && !!draft.model && agentOf(sessionModel) !== agentOf(draft.model);
    const fresh = !target.sessionId || otherAgent;
    const where = remoteOn ? (target.sessionId ? (meta?.cwd ?? "") : (draft.remoteCwd ?? "")) : folder;
    if (!fresh && target.sessionId && (running.has(target.sessionId) || inflight.has(target.sessionId))) {
      const queued = enqueue(held, target.sessionId, { tab: target.id, text: prompt, quotes, files, model: draft.model, effort: draft.effort, mode: permission, cwd: where, remote: remoteOn });
      if (!queued.ok) {
        toast(`The queue holds ${MAX_HELD} messages`);
        return;
      }
      held = queued.queue;
      draft.input = "";
      draft.attachments = [];
      draft.quotes = [];
      comp = comp;
      return;
    }
    if (sending) return;
    if (target.sessionId && otherAgent) toast(`New session on ${nameOf(agentOf(draft.model))}: a thread stays with the agent it started on`);
    const optimistic: ChatEvent = { kind: "user", text: [prompt, ...quotes.map(quoteBlock)].filter(Boolean).join("\n\n") };
    sending = true;
    if (!fresh && target.sessionId) {
      inflight.add(target.sessionId);
      markLive(target.sessionId);
    }
    forgetDraft(target);
    draft.input = "";
    draft.attachments = [];
    draft.quotes = [];
    comp = comp;
    events = [...events, optimistic];
    void scrollToBottom(true);
    try {
      const sid = await api.sendMessage({
        sessionId: fresh ? null : target.sessionId,
        model: draft.model,
        prompt,
        cwd: where,
        effort: draft.effort,
        attachments: files,
        mode: permission,
        remote: remoteOn,
        quotes: quotes.map((q) => ({ project: q.project, id: q.id })),
        worktree: !target.sessionId && !remoteOn ? draft.worktree : undefined,
      });
      if (target.sessionId) inflight.delete(target.sessionId);
      markLive(sid);
      if (target.sessionId && sid !== target.sessionId) {
        running.delete(target.sessionId);
        holdLive.delete(target.sessionId);
        running = new Set(running);
        held = moveHeld(held, target.sessionId, sid);
      }
      rememberModel(draft.model, draft.effort);
      draft.input = "";
      draft.attachments = [];
      draft.quotes = [];
      comp = comp;
      if (fresh) patchTab(target.id, { sessionId: sid, title: (prompt || quotes[0]?.title || "New session").slice(0, 40) });
      else if (sid !== target.sessionId) patchTab(target.id, { sessionId: sid });
      if (activeId === target.id) {
        shown = sid;
        await reload();
      }
      void refreshThreads();
      void drainIfIdle(sid);
    } catch (e) {
      if (target.sessionId) {
        inflight.delete(target.sessionId);
        release(target.sessionId);
      }
      events = events.filter((ev) => ev !== optimistic);
      if (target.sessionId && !fresh && stillWorking(e)) {
        const queued = enqueue(held, target.sessionId, { tab: target.id, text: prompt, quotes, files, model: draft.model, effort: draft.effort, mode: permission, cwd: where, remote: remoteOn });
        if (!queued.ok) {
          draft.input = prompt;
          draft.attachments = files;
          draft.quotes = quotes;
          comp = comp;
          toast(`The queue holds ${MAX_HELD} messages`);
        } else {
          held = queued.queue;
          console.info("queue: still working; the message stays queued", { session: target.sessionId });
        }
        return;
      }
      draft.input = prompt;
      draft.attachments = files;
      draft.quotes = quotes;
      comp = comp;
      toastError(e);
    } finally {
      sending = false;
    }
  }

  function restore(sid: string, h: HeldItem) {
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

  function unhold(sid: string, itemId?: string) {
    const next = itemId ? takeId(held, sid, itemId) : takeHead(held, sid);
    held = next.queue;
    if (next.item) restore(sid, next.item);
  }

  async function sendHeld(sid: string, given?: HeldItem) {
    if (inflight.has(sid)) {
      if (given) held = putFront(held, sid, given);
      return;
    }
    let h = given;
    if (!h) {
      const next = takeHead(held, sid);
      held = next.queue;
      h = next.item;
    }
    if (!h) return;
    inflight.add(sid);
    markLive(sid);
    if (sid === shown) await reload();
    const optimistic: ChatEvent = { kind: "user", text: [h.text, ...h.quotes.map(quoteBlock)].filter(Boolean).join("\n\n") };
    if (sid === shown) {
      events = [...events, optimistic];
      void scrollToBottom(true);
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
        remote: h.remote,
        quotes: h.quotes.map((q) => ({ project: q.project, id: q.id })),
      });
      inflight.delete(sid);
      if (id !== sid) {
        running.delete(sid);
        holdLive.delete(sid);
        markLive(id);
        held = moveHeld(held, sid, id);
        for (const t of tabs.filter((t) => t.sessionId === sid)) patchTab(t.id, { sessionId: id });
      }
      if (shown === sid) {
        shown = id;
        await reload();
      }
      void refreshThreads();
      void drainIfIdle(id);
    } catch (e) {
      inflight.delete(sid);
      release(sid);
      held = putFront(held, sid, h);
      if (shown === sid) events = events.filter((ev) => ev !== optimistic);
      if (stillWorking(e)) {
        console.info("queue: still working; the message stays queued", { session: sid });
        return;
      }
      toastError(e);
    }
  }

  async function drainIfIdle(sid: string) {
    if (inflight.has(sid) || queuePaused.has(sid) || !(held[sid]?.length)) return;
    let status = "";
    try {
      const [m] = await api.getThread(sid);
      status = m.status;
    } catch {
      return;
    }
    if (status === "active" || status === "queued") {
      if (!running.has(sid)) markLive(sid);
      return;
    }
    release(sid);
    void sendHeld(sid);
  }

  async function stop() {
    if (!shown || !running.has(shown)) return;
    const id = shown;
    release(id);
    if ((held[id]?.length ?? 0) > 0) queuePaused = new Set(queuePaused).add(id);
    clearLive();
    await reload();
    void refreshThreads();
    await stopSession(id);
  }

  function dropQueued(sid: string, itemId: string) {
    held = dropId(held, sid, itemId);
    if (!(held[sid]?.length)) queuePaused = new Set([...queuePaused].filter((x) => x !== sid));
  }

  function moveQueued(sid: string, itemId: string, to: number) {
    held = moveId(held, sid, itemId, to);
  }

  function resumeQueue(sid: string) {
    queuePaused = new Set([...queuePaused].filter((x) => x !== sid));
    void drainIfIdle(sid);
  }

  async function sendHeldNow(itemId?: string) {
    const id = shown;
    if (!id) return;
    const next = itemId ? takeId(held, id, itemId) : takeHead(held, id);
    held = next.queue;
    const h = next.item;
    if (!h) return;
    if (running.has(id)) {
      running.delete(id);
      running = new Set(running);
      clearLive();
      await stopSession(id);
      for (let i = 0; i < 40; i++) {
        const t = (await api.listThreads().catch(() => [])).find((x) => x.id === id);
        if (!t || (t.status !== "active" && t.status !== "queued")) break;
        await new Promise((r) => setTimeout(r, 125));
      }
      await new Promise((r) => setTimeout(r, 150));
    }
    for (let i = 0; i < 40 && inflight.has(id); i++) await new Promise((r) => setTimeout(r, 50));
    await sendHeld(id, h);
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

  async function sideTask(fromHeld = false, itemId?: string) {
    const parent = shown;
    if (!parent || tab.kind !== "session" || !tab.sessionId) return;
    let text = "";
    let quotes: PlanQuote[] = [];
    let files: string[] = [];
    let taken: HeldItem | undefined;
    if (fromHeld) {
      const next = itemId ? takeId(held, parent, itemId) : takeHead(held, parent);
      held = next.queue;
      taken = next.item;
      if (!taken) return;
      ({ text, quotes, files } = taken);
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
    const giveBack = () => {
      if (taken) held = putFront(held, parent, taken);
      else {
        comp.input = [text, comp.input].filter(Boolean).join("\n\n");
        comp.attachments = [...files, ...comp.attachments];
        comp.quotes = [...quotes, ...(comp.quotes ?? [])].slice(0, MAX_QUOTES);
        comp = comp;
      }
    };
    if (!comp.model) {
      toast("Pick a model first");
      omnibar?.openModels();
      giveBack();
      return;
    }
    try {
      await launchSide(parent, text, quotes, files);
    } catch (e) {
      giveBack();
      toastError(e);
    }
  }

  async function launchSide(parent: string, text: string, quotes: PlanQuote[], files: string[]): Promise<string> {
    const aimed = await aimAt(text);
    const where = aimed.folder || (remoteOn ? (meta?.cwd ?? "") : folder);
    const prompt = [aimed.text, sideBrief(meta?.title || tab.title, events, aimed.title)].filter(Boolean).join("\n\n");
    const id = await api.sendMessage({
      sessionId: null,
      model: comp.model,
      prompt,
      cwd: where,
      effort: comp.effort,
      attachments: files,
      mode: permission,
      remote: remoteOn,
      quotes: quotes.map((q) => ({ project: q.project, id: q.id })),
      parent,
    });
    running = new Set(running).add(id);
    void refreshThreads();
    const busySame = !aimed.folder && running.has(parent);
    toast(busySame ? "Side task started in this folder too: the two may edit the same files" : `Side task started${aimed.title ? ` in ${aimed.title}` : ""}`);
    return id;
  }

  async function startTask(task: { title: string; brief: string; project?: string }, done: (session: string | null) => void) {
    const parent = shown;
    if (!parent || !comp.model) {
      if (!comp.model) {
        toast("Pick a model first");
        omnibar?.openModels();
      }
      done(null);
      return;
    }
    const aim = task.project ? `@${task.project.replace(/\s+/g, "-")} ` : "";
    try {
      done(await launchSide(parent, `${aim}${task.title}\n\n${task.brief}`, [], []));
    } catch (e) {
      toastError(e);
      done(null);
    }
  }

  async function sendReply(text: string) {
    if (sending || streaming) return;
    const draft = tab.composer;
    const kept = { input: draft.input, attachments: [...draft.attachments], quotes: [...(draft.quotes ?? [])] };
    draft.input = text;
    draft.attachments = [];
    draft.quotes = [];
    comp = comp;
    await send();
    if (kept.input || kept.attachments.length || kept.quotes.length) {
      draft.input = kept.input;
      draft.attachments = kept.attachments;
      draft.quotes = kept.quotes;
      comp = comp;
    }
  }

  async function aimAt(text: string): Promise<{ text: string; folder: string; title: string }> {
    const m = /(^|\s)@([\w.-]+)/.exec(text);
    if (!m) return { text, folder: "", title: "" };
    const want = m[2].toLowerCase();
    const list = await brain.projects().catch(() => []);
    const p = list.find((x) => x.folder && (x.slug.toLowerCase() === want || x.title.toLowerCase().replace(/\s+/g, "-") === want));
    if (!p) return { text, folder: "", title: "" };
    return { text: text.replace(m[0], m[1]).trim(), folder: p.folder, title: p.title };
  }

  function sideBrief(title: string, evs: ChatEvent[], elsewhere: string): string {
    const clip = (s: string, n: number) => (s.length > n ? `${s.slice(0, n).trimEnd()}…` : s);
    const last = (kind: "user" | "assistant") => {
      const e = [...evs].reverse().find((x) => x.kind === kind && x.text.trim());
      return e && "text" in e ? splitQuotes(e.text).text.replace(/\n?\[attached: [^\]]+\]/, "").trim() : "";
    };
    const asked = last("user");
    const said = last("assistant");
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

  async function rewindTo(nth: number, text: string, quotes: PlanQuote[]) {
    const from = shown;
    const target = tab;
    if (!from || target.kind !== "session") return;
    try {
      const m = await api.rewindThread(from, nth);
      patchTab(target.id, { sessionId: m.id, title: m.title });
      if (activeId === target.id) {
        clearLive();
        shown = m.id;
        await reload();
      }
      comp.input = text;
      comp.quotes = quotes.slice(0, MAX_QUOTES);
      comp = comp;
      omnibar?.focus();
      void refreshThreads();
      toast("Rewound. Change the message and send; the full conversation stays in the thread.");
    } catch (e) {
      toastError(e);
    }
  }

  async function reportProblem() {
    const version = await api.appVersion().catch(() => "");
    const body = [
      "**What happened**",
      "",
      "",
      "**What you did just before**",
      "",
      "",
      "**What you expected**",
      "",
      "",
      "---",
      `Parzi ${version || "?"} · ${navigator.userAgent.includes("Windows") ? "Windows" : navigator.userAgent.includes("Mac") ? "macOS" : "Linux"}`,
    ].join("\n");
    const url = `https://github.com/lucas19919/Parzi/issues/new?body=${encodeURIComponent(body)}`;
    await api.openConfirmedUrl(url).catch(toastError);
  }

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

  async function remove(id: string) {
    const title = threads.find((t) => t.id === id)?.title || meta?.title || "this session";
    const ok = await ask(`Delete "${title}" and its transcript?`, { title: "Delete session", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await api.deleteThread(id);
      release(id);
      inflight.delete(id);
      held = takeAll(held, id).queue;
      queuePaused = new Set([...queuePaused].filter((x) => x !== id));
      queuePaused = queuePaused;
      if (pinnedDocs[id]) {
        const { [id]: _, ...rest } = pinnedDocs;
        pinnedDocs = rest;
      }
      const open = tabs.filter((t) => t.sessionId === id).map((t) => t.id);
      for (const tid of open) closeTab(tid);
      void refreshThreads();
      toast("Session deleted");
    } catch (e) {
      toastError(e);
    }
  }

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

  function revealLive() {
    revealing = 0;
    if (!pending) return;
    let cut = Math.min(pending.length, Math.max(3, Math.ceil(pending.length / 6)));
    if (cut < pending.length && /[\uD800-\uDBFF]/.test(pending[cut - 1])) cut++;
    live += pending.slice(0, cut);
    pending = pending.slice(cut);
    if (pending) revealing = requestAnimationFrame(revealLive);
  }

  const titleOf = (session: string) => threads.find((t) => t.id === session)?.title || "Agent";

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
      questions = [...questions.filter((q) => q.key !== e.key), { key: e.key, session: e.session, question: e.question, options: e.options, details: e.details, multi: e.multi }];
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
      if (e.kind === "error" && /exited before/i.test(e.error) && (held[e.session]?.length ?? 0) > 0) {
        queuePaused = new Set(queuePaused).add(e.session);
      }
      release(e.session);
      for (const k of [...approvals, ...questions, ...requests].filter((x) => x.session === e.session).map((x) => x.key)) settle(k);
      void refreshThreads();
      const next = e.kind === "done" && !inflight.has(e.session) && !queuePaused.has(e.session) && !!held[e.session]?.length;
      if (e.session === shown) {
        if (next) clearLive();
        else void reload(clearLive);
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
    if (e.kind === "tool_result" && e.name === "ui.show_artifact" && e.ok) announce(e.session, "published an artifact", "success", { chime: false });
    if (e.session !== shown) return;
    if (e.kind === "text") {
      pending += e.text;
      if (!revealing) revealing = requestAnimationFrame(revealLive);
    } else if (e.kind === "reasoning") {
      liveReasoning += e.text;
    } else if (e.kind === "todo") {
      liveTodo = e.todo;
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
      closeTab(activeId);
    }
    else if (name === "ctrl+comma") {
      if (tab.kind === "settings") closeTab(activeId);
      else if (cover === "settings") clearCover();
      else openSettings();
    } else if (name === "ctrl+j" || name === "ctrl+shift+a") toggleInfo();
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
      const strip = tabs;
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
              : mod && ["p", "k", "t", "w", "l", "h", "j"].includes(key)
                  ? `ctrl+${key}`
                  : "";
    if (name && shortcut(name)) {
      e.preventDefault();
    } else if (key === "escape" && !e.defaultPrevented && !$covered) {
      if (immersive) void setImmersive(false);
      else if (coverOn) clearCover();
      else if (tab.kind === "settings") closeTab(activeId);
    }
  }

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
    else if (cmd.what === "plan" || cmd.what === "work" || cmd.what === "tasks") showPlan();
    else if (cmd.what === "home") newSession();
  }

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
    const unResync = onResync(() => void refreshThreads());
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
    const unActivity = watchActivity();
    selectTab(activeId);
    if (!$onboarded) setupOpen = true;
    void warmPages();
    const onBg = (e: Event) => (bg = (e as CustomEvent<string>).detail);
    document.addEventListener("parzi:bg", onBg);
    const onDefaultMode = (e: Event) => {
      policy = (e as CustomEvent<string>).detail ?? "";
      permission = permissionFor(policy);
    };
    document.addEventListener("parzi:default-mode", onDefaultMode);
    const onOpenPage = (e: Event) => {
      const url = (e as CustomEvent<string>).detail ?? "";
      if (!url) return;
      if (tab.kind === "session" && tab.sessionId) openInSession(url, tab.sessionId);
      else openPage(url);
    };
    document.addEventListener("parzi:open-page", onOpenPage);
    const onOpenArtifact = (e: Event) => {
      const doc = (e as CustomEvent<ArtifactDoc>).detail;
      if (doc?.content) openArtifact(doc);
    };
    document.addEventListener("parzi:open-artifact", onOpenArtifact);
    const onPinNote = (e: Event) => {
      const path = (e as CustomEvent<string>).detail ?? "";
      if (path) void pinNote(path);
    };
    document.addEventListener("parzi:pin-note", onPinNote);
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
      .then((list) => {
        threads = list;
        noteThreads(list);
      })
      .catch((e) => toast(`Couldn't load sessions: ${e}`, true));
    void loadRemoteInfo().catch(() => {});
    const unRemote = onRemoteStatus((s) => {
      if (s.state === "updating" || s.state === "ready") toast(s.detail);
      else if (s.state === "error") toast(s.detail, true);
      if (s.state === "up" || s.state === "resync" || s.state === "ready") {
        void refreshThreads();
        if (remoteOn) void refreshRemoteBoard().catch(() => {});
        if (isRemote(shown)) void reload();
      }
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
      unActivity();
      unClear();
      document.removeEventListener("parzi:default-mode", onDefaultMode);
      document.removeEventListener("parzi:open-page", onOpenPage);
      document.removeEventListener("parzi:open-artifact", onOpenArtifact);
      document.removeEventListener("parzi:pin-note", onPinNote);
      unRun.then((f) => f()).catch(() => {});
      unDesk.then((f) => f()).catch(() => {});
      unResync.then((f) => f()).catch(() => {});
      unRemote.then((f) => f()).catch(() => {});
      for (const un of [unPage, unOpen, unKey, unAsk, unDownload]) un.then((f) => f()).catch(() => {});
      document.removeEventListener("parzi:bg", onBg);
    };
  });
</script>


<svelte:window on:keydown={onKey} on:pagehide={persistDrafts} on:beforeunload={persistDrafts} bind:innerWidth />

<div class="shell">
  <DefaultArt bg={showWallpaper ? bg : ""} />

  {#if !immersive}
    <TopBar
      {tabs}
      activeTabId={activeId}
      {threads}
      {liveModel}
      on:select={(e) => selectTab(e.detail.id)}
      on:close={(e) => closeTab(e.detail.id)}
      on:move={(e) => moveTab(e.detail.id, e.detail.to)}
    on:newTab={newSession}
      on:manager={() => showCover("sessions")}
      on:settings={() => openSettings()}
      panelable={tab.kind === "session"}
      panelOpen={infoShown}
      panelCount={familyActive}
      on:panel={() => toggleInfo()}
    />
  {/if}

  <div class="content">
    <main>
    {#if tab.kind === "settings"}
      <div class="fill col" in:fly={{ y: 8, ...motion }}>
        <PanelHeader title="Settings" on:close={() => closeTab(activeId)} />
        <Settings bind:section={settingsSection} on:close={() => closeTab(activeId)} on:setupRemote={openRemoteSetup} onSetup={openSetup} onReport={reportProblem} />
      </div>
    {:else if tab.kind === "preview"}
      <div class="fill col" in:fade={{ duration: 150 }}>
        <PanelHeader title={tab.title} on:close={() => closeTab(activeId)} />
        <PreviewView which={tab.url ?? "omnibar"} />
      </div>
    {:else if tab.kind === "artifact" && tab.doc}
      <div class="fill col ghost" in:fade={{ duration: 150 }}>
        <PanelHeader title={tab.title} on:close={() => closeTab(activeId)} />
        {#key `${tab.id}:${tab.doc.version}`}
          <ArtifactCard data={tab.doc} reader />
        {/key}
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
      {#if tab.kind === "session" && (hasSession || infoShown || coverOn)}
        <div class="stagerow" class:solo={!hasSession && !coverOn} out:fade|local={{ duration: motion.duration }}>
          {#if hasSession || coverOn}
          <section class="session" out:fade|local={{ duration: motion.duration }}>
            {#if coverOn || parent}
              <button
                class="back icon-btn"
                aria-label={coverOn ? "Back" : `Back to ${parent?.title || "Untitled"}`}
                use:tooltip={coverOn ? "Back" : `Back to ${parent?.title || "Untitled"}`}
                transition:fly|global={{ x: 28, ...motion }}
                on:click={() => (coverOn ? clearCover() : parent && openSession(parent.id))}
              >
                <Icon name="arrowLeft" />
              </button>
            {/if}
            {#if coverOn && cover === "settings"}
              <div class="cover" transition:fly|global={{ y: 8, ...motion }}>
                <Settings bind:section={settingsSection} on:setupRemote={openRemoteSetup} onSetup={openSetup} onReport={reportProblem} />
              </div>
            {:else if coverOn && cover === "sessions"}
              <div class="cover" transition:fly|global={{ y: 8, ...motion }}>
                <ManagerView
                  {threads}
                  on:openSession={(e) => {
                    clearCover();
                    openSession(e.detail.id);
                  }}
                  on:deleteSession={(e) => remove(e.detail.id)}
                  on:newSession={() => {
                    clearCover();
                    newSession();
                  }}
                />
              </div>
            {:else if coverOn && cover === "doc"}
              <div class="cover" transition:fly|global={{ y: 8, ...motion }}>
                {#if pinnedDoc}
                  <div class="doc-tools">
                    <button class="tool" aria-label="Document actions" use:tooltip={"Document actions"} on:click={toggleDocMenu} bind:this={docMenuBtn}><Icon name="more" size={12} /></button>
                  </div>
                  {#if docMenuOpen}
                    <div class="menu-pop" style={docMenuStyle} use:popover={{ anchor: docMenuBtn, close: () => (docMenuOpen = false) }}>
                      <button class="opt" on:click={copyPinnedDoc}><Icon name={docCopied ? "check" : "copy"} size={12} /><span class="name">{docCopied ? "Copied" : "Copy"}</span></button>
                      <button class="opt" on:click={savePinnedDoc}><Icon name="download" size={12} /><span class="name">Save as file</span></button>
                    <button class="opt" on:click={openPinnedDocFolder}><Icon name="folder" size={12} /><span class="name">Open in file explorer</span></button>
                    </div>
                  {/if}
                  {#key `${pinnedDoc.id}:${pinnedDoc.version}`}
                    <ArtifactCard data={pinnedDoc} reader bare />
                  {/key}
                {/if}
              </div>
            {:else if hasSession}
            <div class="workarea">
              <div class="scroll" style:--dock="{dockHeight}px" bind:this={scrollEl} use:follow.attach>
                <Thread
                  {events}
                  liveText={live}
                  {liveReasoning}
                {liveTodo}
                  {liveTools}
                  {approval}
                  question={openQuestion}
                  request={openRequest}
                  askerTitle={threads.find((t) => t.id === (openQuestion?.session ?? openRequest?.session ?? approval?.session ?? shown))?.title ?? ""}
                  agentName={nameOf(threads.find((t) => t.id === shown)?.model.split("/")[0] ?? "")}
                  {streaming}
                  session={shown ?? ""}
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
                  on:reply={(e) => void sendReply(e.detail.text)}
                on:rewind={(e) => void rewindTo(e.detail.nth, e.detail.text, e.detail.quotes)}
                on:pickModel={() => omnibar?.openModels()}
                  on:startTask={(e) => void startTask(e.detail.task, e.detail.done)}
                  on:openSession={(e) => openSession(e.detail.id)}
                >
                  <PlanInline
                    project={planProject}
                    session={shown ?? ""}
                    {threads}
                    {running}
                    {approvals}
                    questions={asks}
                    quotes={comp.quotes ?? []}
                    force={planForcedNow}
                    bind:visible={planInline}
                    on:quote={(e) => quoteItem(e.detail)}
                    on:unquote={(e) => unquoteItem(e.detail)}
                    on:close={hidePlan}
                  />
                </Thread>
              </div>
            </div>
            {/if}
            {#if hasSession && !coverOn}
              <div class="thread-fade" style:--dock="{dockHeight}px" aria-hidden="true"></div>
            {/if}
          </section>
          {/if}
          <SessionInfo
            session={shown ?? ""}
            project={planProject}
            projectTitle={planTitle}
            {folder}
            {branch}
            remoteHost={remoteOn ? ($remoteInfo?.host ?? "") : ""}
            contextUsed={context.used}
            contextLimit={context.limit}
            {compacting}
            {streaming}
            {threads}
            {running}
            {approvals}
            questions={asks}
            {planInline}
            doc={pinnedDoc}
            width={infoWidth}
            open={infoShown}
            on:resize={resizeInfo}
            on:openDoc={() => showCover("doc")}
            on:closeDoc={closePinnedDoc}
            tokensIn={meta?.tokens_in ?? 0}
            tokensOut={meta?.tokens_out ?? 0}
            costUsd={meta?.cost_usd ?? 0}
            provider={meta?.model.split("/")[0] ?? ""}
            on:openSession={(e) => openSession(e.detail.id)}
            on:openUrl={(e) => openPage(e.detail.url)}
            on:compact={compact}
            on:plan={showPlan}
          />
        </div>
      {/if}

      {#if !coverOn}
      <div
        class="composer"
        class:docked={hasSession}
        class:browse={comp.mode === "search"}
        style:--shift="{shift}px"
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
          bind:worktree={comp.worktree}
          {folder}
          folderLocked={!!tab.sessionId}
          started={!!tab.sessionId}
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
          queue={heldRows}
          queuePaused={queueIsPaused}
          on:send={send}
          on:unhold={(e) => shown && unhold(shown, e.detail.id)}
          on:dropHeld={(e) => shown && dropQueued(shown, e.detail.id)}
          on:moveHeld={(e) => shown && moveQueued(shown, e.detail.id, e.detail.to)}
          on:resumeQueue={() => shown && resumeQueue(shown)}
          on:side={() => void sideTask(false)}
          on:sideHeld={(e) => void sideTask(true, e.detail.id)}
          on:sendHeldNow={(e) => void sendHeldNow(e.detail.id)}
          canSide={tab.kind === "session" && !!tab.sessionId}
          on:browse={(e) => browse(e.detail.url)}
          on:stop={stop}
          on:command={(e) => onCommand(e.detail.name)}
          on:unavailable={() => openSettings(remoteOn ? "remote" : "providers")}
          on:folder={(e) => patchTab(tab.id, { cwd: e.detail.path })}
          on:project={(e) => loadProject(e.detail.path ?? tab.cwd ?? folder)}
        />
      </div>
      {/if}
    {/if}
    </main>
  </div>

  <ImageViewer />
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
  />

  {#if setupOpen}
    <Onboarding startStep={setupStep} server={setupServer} on:close={() => (setupOpen = false)} />
  {/if}


  
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
    margin: 0 var(--gap);
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
  }
  .fill.col {
    flex-direction: column;
  }
  .fill.ghost {
    background: none;
  }
  .stagerow {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .stagerow.solo {
    justify-content: flex-end;
  }
  .session {
    position: relative;
    min-width: 0;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap);
    overflow: hidden;
    background: none;
    border: 1px solid var(--line);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
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

  .back {
    position: absolute;
    top: calc(var(--space) * 2);
    right: calc(var(--space) * 4);
    z-index: 5;
  }
  .doc-tools {
    position: absolute;
    top: calc(var(--space) * 2);
    right: calc(var(--space) * 4 + var(--space) * 7 + var(--space));
    z-index: 5;
  }
  .doc-tools .tool {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .doc-tools .tool:hover {
    background: transparent;
    color: var(--text);
  }
  .menu-pop .opt {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    height: 32px;
    padding: 0 calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .menu-pop .opt:hover {
    background: transparent;
  }
  .menu-pop .opt .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
  }


  .cover :global(nav),
  .cover :global(header),
  .cover :global(.bar),
  .cover :global(.page-bar) {
    anchor-name: --cover-bar;
    padding-right: calc(var(--space) * 4 + var(--space) * 7 + var(--space) * 2);
  }
  .session:has(.cover) .back {
    position-anchor: --cover-bar;
    top: anchor(center, calc(var(--space) * 2 + 14px));
    translate: 0 -50%;
  }
  .cover {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* Doc covers start one message gap below the top, like thread turns. */
  .cover :global(.art.reader .doc) {
    padding-top: calc(var(--space) * 3);
  }
  .scroll {
    flex: 1;
    overflow-x: hidden;
    overflow-y: auto;
    padding-bottom: calc(var(--dock) + calc(var(--space) * 8));
    user-select: text;
  }

  .thread-fade {
    pointer-events: none;
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: calc(var(--dock, 140px) + 28px);
    z-index: 15;
    background: linear-gradient(to bottom, transparent, var(--bg) 55%);
  }

  .composer {
    position: absolute;
    left: 50%;
    bottom: 46%;
    z-index: 20;

    width: min(var(--read), calc(100% - 64px - 2 * var(--shift, 0px)));
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
    transition: width 300ms var(--ease-out), bottom 300ms var(--ease-out), transform 300ms var(--ease-out);
  }

  .composer.browse:not(.docked) {
    top: 0;
    bottom: auto;
  }
  .composer.docked {
    bottom: 14px;

    width: min(var(--read), calc(100% - 64px - 2 * var(--shift, 0px)));
    transform: translate(calc(-50% - var(--shift, 0px)), 0);
    animation: dock-in 260ms var(--ease-out);
  }
  @keyframes dock-in {
    from {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .composer.docked {
      animation: none;
    }
  }
  .composer.docked :global(.box) {
    background: var(--bg);
  }
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 1200;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    pointer-events: none;
  }
  .toast {
    max-width: 360px;
    padding: calc(var(--space) * 2) calc(var(--space) * 4);
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
    gap: calc(var(--space) * 2);
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
    margin-left: calc(var(--space) * 2);
    border-radius: var(--radius-sm);
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
    padding: var(--space) calc(var(--space) * 3);
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
    padding: var(--space) calc(var(--space) * 2) var(--space) calc(var(--space) * 3);
    white-space: nowrap;
  }
  .toasts.chrome .toast:not(.acts) {
    padding-right: calc(var(--space) * 3);
  }
</style>
