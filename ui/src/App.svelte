<script lang="ts">
  import { DUR } from "./lib/motion";
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api, brain, deskSync, onBrowser, onBrowserKey, onBrowserOpen, onDesk, onResync, onRunEvent,
    type AgentRequest, type ChatEvent, type ComposerMode, type PageEvent, type Question, type SessionMeta, type UiEvent,
  } from "./lib/api";
  import { deskAnswer, onBrowserAsk } from "./lib/api";
  import { defineActions, describeActions, runAction, type AppAction } from "./lib/actions";
  import { click as clickControl, screen as readScreen, type as typeControl } from "./lib/appScreen";
  import { addPrompt, dropTabPrompts } from "./lib/browserState";
  import { closedTabs, recordFavicon } from "./lib/browserData";
  import { popReopenable, pushClosed } from "./lib/pageRules";
  import TopBar from "./lib/TopBar.svelte";
  import DefaultArt from "./lib/DefaultArt.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import PanelHeader from "./lib/PanelHeader.svelte";
  import SessionInfo from "./lib/SessionInfo.svelte";
  import SessionPins from "./lib/SessionPins.svelte";
  import type { Pin } from "./lib/pins";
  import FileReader, { FILE_MAX, FILE_MIN, fileDoc as diskDoc, imageDoc, isImagePath } from "./lib/FileReader.svelte";
  import ImageViewer from "./lib/ImageViewer.svelte";
  import Thread from "./lib/Thread.svelte";
  import PageView from "./lib/PageView.svelte";
  import ManagerView from "./lib/ManagerView.svelte";
  import ArtifactCard from "./lib/widgets/ArtifactCard.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import { loadTabs, onboarded, recordVisit, saveTabs, titleVisit } from "./lib/browserData";
  import Settings from "./lib/Settings.svelte";
  import Icon from "./lib/Icon.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { isRemote, loadRemoteInfo, onRemoteStatus, refreshRemoteBoard, remote, remoteBoard, remoteInfo } from "./lib/remote";
  import { permissionFor } from "./lib/permission";
  import { provideClearSessions } from "./lib/sessions";
  import { applyThemeCss, refreshBackground } from "./lib/theme";
  import { coalesce } from "./lib/threadList";
  import { checkForUpdatesSoon } from "./lib/updateStore";
  import { artifactTab, displayTitle, ensureComposer, hostOf, isExplicitUrl, pageTab, placeTab, rememberModel, sessionTab, toAddress, type ArtifactDoc, type Tab, type TabComposerState } from "./lib/tabs";

  import { activity, sessionMarks, watchActivity, type SessionMark } from "./lib/activity";
  import { splitQuotes } from "./lib/oldQuotes";
  import { toast, toastError, toasts, notify, chime, dismissToast, keyedToast } from "./lib/toast";
  import { freshNotice } from "./lib/notifyPrefs";
  import { effortsFor, nameOf } from "./lib/providerRows";
  import { follower } from "./lib/follow";
  import { draftKey, flushDrafts, loadDrafts, sameDraft, saveDrafts, withDraft } from "./lib/drafts";
  import { flushHeld, heldLabel, loadHeld, MAX_HELD, saveHeld, stillWorking, type HeldItem } from "./lib/held";
  import { echoOf, initialSends, isBusy, isPaused, isWorking, phaseOf, step, tabKey, workingSet, type Effect, type Outcome, type SendEvent, type Sends, type Step } from "./lib/sendState";
  import { covered } from "./lib/overlay";
  import { tooltip } from "./lib/tooltip";
  import type { Approval, LiveTool } from "./lib/live";
  import { deskRows, type DeskCmd } from "./lib/desk";
  import { attachedPaths } from "./lib/format";

  // Phones are emulated through WebView2's device mode, so only on Windows.
  const phonesWork = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
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
  // Pages open over a session now; only an agent's own pages are tabs, so a
  // page tab the person had open is not brought back.
  const restoredTabs = (restored?.tabs ?? []).filter((t) => (t as Partial<Tab>).kind !== "page" || !!(t as Partial<Tab>).owner);
  let tabs: Tab[] = oneEmptyDraft(
    (restoredTabs.length ? restoredTabs : [sessionTab()]).map((t) => {
      const c = ensureComposer((t as Partial<Tab>).composer);
      if (c.model === "auto") c.model = "";
      return { ...t, title: displayTitle((t as Partial<Tab>).title, (t as Partial<Tab>).kind === "page" ? "New page" : "New session"), composer: c };
    }),
  );
  let activeId = restored?.active && tabs.some((t) => t.id === restored.active) ? restored.active : tabs[0].id;
  let threads: SessionMeta[] = [];
  let bg = "";

  let shown: string | null = null;
  let meta: SessionMeta | null = null;
  /** The thread as the store has it; `events` adds the message still on its way. */
  let stored: ChatEvent[] = [];
  let approvals: Approval[] = [];
  let questions: Question[] = [];
  let requests: AgentRequest[] = [];
  /** How a session's last run ended, kept until someone opens it. */
  let unseen: Record<string, SessionMark> = {};
  $: sessionMarks.set(
    Object.fromEntries<SessionMark>([
      ...Object.entries(unseen),
      ...[...working].map((id) => [id, "running"] as const),
      ...[...approvals, ...questions, ...requests].map((x) => [x.session, "waiting"] as const),
    ]),
  );
  let context = { used: 0, limit: 0 };
  let compacting = false;
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTodo: unknown = null;
  let liveArtifacts: Extract<UiEvent, { kind: "artifact" }>[] = [];
  let liveTools: LiveTool[] = [];
  let pending = "";
  let revealing = 0;

  let model = "";
  let warmed = "";
  let permission = permissionFor("");
  let policy = "";
  let mode: ComposerMode = "build";
  // Sending, queueing and stopping, per session (lib/sendState.ts). `act` is
  // the only writer; in functions read `sends` itself, since the values
  // derived here only catch up after the update.
  let sends: Sends = initialSends(loadHeld());
  $: working = workingSet(sends);
  $: sending = phaseOf(sends, tab.sessionId ?? tabKey(tab.id)) === "sending";
  $: heldRows = (tab.kind === "session" && shown ? sends.queue[shown] ?? [] : []).map((item) => ({
    id: item.id,
    text: heldLabel(item),
    detail: item.files.length ? `${item.files.length} file${item.files.length === 1 ? "" : "s"}` : "",
  }));
  $: queueIsPaused = !!shown && isPaused(sends, shown);
  // The message just sent shows until the store has it.
  $: echoText = echoOf(sends, shown ?? (tab.kind === "session" && !tab.sessionId ? tabKey(tab.id) : ""))?.text ?? null;
  $: events = echoText === null ? stored : [...stored, { kind: "user", text: echoText } as ChatEvent];
  /** The session whose ended turn stays on screen until its saved reply is in. */
  let closing = "";

  function act(ev: SendEvent): Step {
    const r = step(sends, ev, Date.now());
    if (r.state.queue !== sends.queue) saveHeld(r.state.queue);
    if (r.state !== sends) sends = r.state;
    for (const fx of r.effects) perform(fx);
    return r;
  }

  function perform(fx: Effect) {
    if (fx.do === "deliver") void sendHeld(fx.key, fx.item);
    else if (fx.do === "kill") void stopSession(fx.key);
    else if (fx.do === "retry") {
      const { key, tries } = fx;
      setTimeout(() => {
        void api
          .getThread(key)
          .then(([m]) => m.status, () => "active")
          .then((status) => act({ type: "woke", key, tries, status }));
      }, fx.ms);
    }
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
    if (t.kind !== "session" || c.input.trim() || c.attachments.length) return;
    const d = drafts[draftKey(t)];
    if (!d) return;
    c.input = d.input;
    c.attachments = [...d.attachments];
  }

  function keepDraft(t: Tab | undefined, d: { input: string; attachments: string[] }) {
    if (t?.kind !== "session") return;
    const key = draftKey(t);
    if (sameDraft(drafts[key], d)) return;
    drafts = withDraft(drafts, key, d);
    saveDrafts(drafts);
  }

  function forgetDraft(t: Tab) {
    keepDraft(t, { input: "", attachments: [] });
  }

  $: keepDraft(
    tabs.find((t) => t.id === compFor && t.composer === comp),
    { input: comp.input, attachments: comp.attachments },
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
      if (t.id === shown || !(t.status === "active" || t.status === "queued" || working.has(t.id))) continue;
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

  let infoOpen = true;
  try {
    infoOpen = localStorage.getItem("parzi.info.open") !== "0";
  } catch {}
  let infoWidth = 264;
  try {
    const w = parseInt(localStorage.getItem("parzi.info.width") ?? "", 10);
    if (w >= 220 && w <= 520) infoWidth = w;
  } catch {}
  let fileWidth = 420;
  try {
    const w = parseInt(localStorage.getItem("parzi.file.width") ?? "", 10);
    // 264 was the first default, the same width as the session column.
    if (w >= FILE_MIN && w <= FILE_MAX && w !== 264) fileWidth = w;
  } catch {}
  let innerWidth = 1280;
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
  $: streaming = !!shown && (working.has(shown) || closing === shown);
  // Subagents of the session on screen that are still at work, with the
  // step each is on, for the activity in its thread.
  $: needsYou = new Set([...approvals, ...questions, ...requests].map((x) => x.session));
  $: liveHelpers = shown
    ? threads
        .filter((t) => t.parent_id === shown && (working.has(t.id) || needsYou.has(t.id)))
        .map((t) => ({ id: t.id, title: t.title || "Untitled", need: needsYou.has(t.id), step: $activity.get(t.id)?.label ?? "" }))
    : [];
  $: hasSession = tab.kind === "session" && (!!shown || sending || events.length > 0);
  $: remoteOn = tab.kind === "session" && tab.sessionId ? isRemote(tab.sessionId) : !!$remoteInfo && (comp.remote ?? preferRemote);
  $: folder = remoteOn ? "" : tab.kind === "session" && tab.sessionId ? (meta?.cwd ?? "") : (tab.cwd ?? "");
  $: approval = approvals.find((a) => a.session === shown) ?? null;
  // A plan the agent proposed waits on the session (meta.proposal) and shows
  // where a question would; its answer goes as the next message. A question
  // from a tool call comes first.
  $: openQuestion = questions.find((q) => q.session === shown) ?? null;
  $: proposal = !openQuestion && !!shown && meta?.id === shown && meta.proposal?.items?.length ? meta.proposal : null;
  // A proposal that arrives brings the thread down to it, once.
  let seenProposal = "";
  $: proposalFor = proposal ? (shown ?? "") : "";
  $: if (proposalFor !== seenProposal) {
    seenProposal = proposalFor;
    if (seenProposal) void scrollToBottom(true);
  }
  $: openRequest = requests.find((r) => r.session === shown) ?? null;
  $: asks = [...questions, ...requests.map((r) => ({ key: r.key, session: r.session, question: r.request, options: [] }))];
  $: parent = meta?.parent_id ? (threads.find((t) => t.id === meta?.parent_id) ?? null) : null;
  let lastFolder = "@@none@@";
  $: if (folder !== lastFolder) {
    lastFolder = folder;
    void loadBranch(folder);
    void loadProject(folder);
  }
  // The page open over a session is on the desk too, under the id the desk
  // gave it, so the command line can find, focus and close it.
  $: deskTabs = coverPage ? [...tabs, coverPage] : tabs;
  $: deskActive = coverPageOn && coverPage ? coverPage.id : activeId;
  $: tabsKey = JSON.stringify([deskActive, deskTabs.map((t) => [t.id, t.kind, t.title, t.url ?? "", t.sessionId ?? "", t.owner ?? "", t.group ?? "", t.cwd ?? ""])]);
  let lastTabsKey = "";
  $: if (tabsKey !== lastTabsKey) {
    lastTabsKey = tabsKey;
    syncDesk(deskTabs, deskActive);
    saveTabs(tabs, activeId);
  }
  $: pageVisible = tab.kind === "page";
  $: toastsUp = (pageVisible || (coverOn && cover === "page")) && !immersive;
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

  function noteThreads(list: { id: string; status: string }[]) {
    act({ type: "seen", list: list.map(({ id, status }) => ({ id, status })) });
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

  let cover: "settings" | "sessions" | "page" | "file" | null = null;
  let coverFor = "";
  // A link or a pin opens full screen over the thread, with the composer
  // still floating over it; Settings and Sessions cover it without one.
  let coverPage: Tab | null = null;
  let coverDoc: ArtifactDoc | null = null;
  /** The link or pin the open page or file came from. */
  let coverTarget = "";
  $: coverOn = cover !== null && tab.kind === "session" && activeId === coverFor;
  $: coverComposer = coverOn && (cover === "page" || cover === "file");
  $: coverPageOn = coverOn && cover === "page" && !!coverPage;
  // Full screen in the page over a session (a video, say) takes the whole
  // window: the composer and Back give way until it ends.
  $: pageFull = immersive && coverPageOn;
  let lastCoverPageOn = false;
  $: if (coverPageOn !== lastCoverPageOn) {
    lastCoverPageOn = coverPageOn;
    if (!coverPageOn && immersive && !pageVisible) void setImmersive(false);
  }
  $: showWallpaper = tab.kind === "session" && !hasSession && !coverOn;
  $: overArt = showWallpaper && !!bg;
  $: onHome = showWallpaper;

  /* For a moment after the window comes to or leaves Home (or Home goes
     onto or off the wallpaper), the glass, the outlines and the colours
     ease to their new look (.art-shift) instead of switching at once,
     alongside the picture's fade (DefaultArt). */
  let artShift = false;
  let wasHome = "false:false";
  let artShiftTimer: ReturnType<typeof setTimeout> | undefined;
  $: shiftArt(`${onHome}:${overArt}`);
  function shiftArt(on: string) {
    if (on === wasHome) return;
    wasHome = on;
    if (reduced) return;
    artShift = true;
    clearTimeout(artShiftTimer);
    // Counted from the first frame that shows the change: opening a session
    // can hold the page a moment, and the ease must not be cut short.
    requestAnimationFrame(() => {
      clearTimeout(artShiftTimer);
      artShiftTimer = setTimeout(() => (artShift = false), 620);
    });
  }

  function clearCover() {
    cover = null;
    coverFor = "";
    dropCoverPage();
    coverDoc = null;
    coverTarget = "";
  }

  /** The page over the thread is closed, not hidden: its native view would
   *  otherwise stay drawn over whatever comes next. */
  function dropCoverPage() {
    if (!coverPage) return;
    const id = coverPage.id;
    coverPage = null;
    dropTabPrompts(id);
    api.browserClose(id).catch(toastError);
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

  function showCover(kind: "settings" | "sessions" | "page" | "file") {
    if (kind !== "page") dropCoverPage();
    if (kind !== "file") coverDoc = null;
    if (kind !== "page" && kind !== "file") coverTarget = "";
    const id = sessionHost();
    cover = kind;
    coverFor = id;
    if (activeId !== id) selectTab(id);
  }

  function reopenClosed() {
    // Sessions of another machine ("r:", "m:" ids) are not in this list, so they are taken as still there.
    const exists = (id: string) => /^[a-z]:/.test(id) || threads.some((t) => t.id === id);
    const [last, rest] = popReopenable($closedTabs, exists);
    closedTabs.set(rest);
    if (!last) return;
    if (last.sessionId) openSession(last.sessionId);
    else openFullPage(last.url);
  }

  $: sessionProject = (hasSession ? meta?.project : "") || sideMeta?.project || project?.slug || "";
  $: projectTitle = project && project.slug.toLowerCase() === sessionProject.toLowerCase() ? project.title : "";
  $: infoShown = tab.kind === "session" && infoOpen && innerWidth >= 900;
  // Browser tabs get the side column on their own: there is not always a
  // session behind a page, so the column shows the current context instead
  // of waiting for one.
  $: pageOwner = tab.kind === "page" ? (tab.owner ?? tab.group ?? null) : null;
  $: pageOwnerMeta = pageOwner ? (threads.find((t) => t.id === pageOwner) ?? null) : null;
  $: pageInfoShown = tab.kind === "page" && infoOpen && innerWidth >= 900;
  $: sideId = pageOwner ?? shown ?? "";
  $: sideMeta = pageOwnerMeta ?? meta;
  // A file open in the side bar takes its second column, beside the cards.
  // It gives way before the thread does: never wider than leaves the
  // thread 480px.
  $: readerShown = (infoShown || pageInfoShown) && !!fileDoc;
  $: readerWidth = Math.max(FILE_MIN, Math.min(fileWidth, innerWidth - infoWidth - 480));

  $: shift = Math.round(
    ((infoShown || pageInfoShown ? infoWidth + 8 : 0) + (readerShown ? readerWidth + 8 : 0)) / 2,
  );

  function toggleInfo(open = !(infoShown || pageInfoShown)) {
    infoOpen = open;
    try {
      localStorage.setItem("parzi.info.open", open ? "1" : "0");
    } catch {}
    if (open && (hasSession || pageOwnerMeta) && innerWidth < 900) toast("Widen the window to see the session column");
  }

  function resizeInfo(e: { detail: { width: number } }) {
    infoWidth = Math.min(520, Math.max(220, Math.round(e.detail.width)));
    try {
      localStorage.setItem("parzi.info.width", String(infoWidth));
    } catch {}
  }

  function resizeFile(e: { detail: { width: number } }) {
    fileWidth = Math.min(FILE_MAX, Math.max(FILE_MIN, Math.round(e.detail.width)));
    try {
      localStorage.setItem("parzi.file.width", String(fileWidth));
    } catch {}
  }

  async function renamed(id: string) {
    await refreshThreads();
    const t = threads.find((x) => x.id === id);
    if (!t?.title) return;
    tabs = tabs.map((x) => (x.sessionId === id && x.title !== t.title ? { ...x, title: t.title } : x));
    if (meta?.id === id) meta = { ...meta, title: t.title };
  }

  async function moved(id: string) {
    await refreshThreads();
    const t = threads.find((x) => x.id === id);
    if (t && meta?.id === id) meta = { ...meta, cwd: t.cwd, project: t.project };
  }

  function openRemoteSetup() {
    setupStep = 2;
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
    revealAt = 0;
    thinking = false;
    pending = "";
    live = "";
    liveReasoning = "";
    liveTodo = null;
    liveTools = [];
    liveArtifacts = [];
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
    stored = [];
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

  // Keep recently opened threads warm in memory so switching sessions is
  // instant without fading out or jumping; the fresh disk read reconciles
  // quietly right behind it.
  const THREAD_CACHE_MAX = 24;
  const threadCache = new Map<string, { meta: SessionMeta; events: ChatEvent[] }>();
  function cacheThread(id: string, m: SessionMeta, ev: ChatEvent[]) {
    if (!id) return;
    threadCache.delete(id);
    threadCache.set(id, { meta: m, events: ev });
    while (threadCache.size > THREAD_CACHE_MAX) {
      const oldest = threadCache.keys().next().value;
      if (oldest) threadCache.delete(oldest);
      else break;
    }
  }
  function dropCachedThread(id: string) {
    threadCache.delete(id);
  }
  function eventsDiffer(a: ChatEvent[], b: ChatEvent[]): boolean {
    if (a.length !== b.length) return true;
    if (!a.length) return false;
    const la = a[a.length - 1];
    const lb = b[b.length - 1];
    return la.kind !== lb.kind || JSON.stringify(la) !== JSON.stringify(lb);
  }

  async function showSession(id: string) {
    const my = ++navSeq;
    // The tab this load is for: switching to a page tab doesn't bump navSeq,
    // so a late reply must also check the tab is still the one in front.
    const at = activeId;
    shown = id;
    if (unseen[id]) unseen = Object.fromEntries(Object.entries(unseen).filter(([k]) => k !== id));
    clearLive();
    const cached = threadCache.get(id);
    if (cached) {
      meta = cached.meta;
      showStored(id, cached.events);
      context = { used: cached.meta.context_tokens ?? 0, limit: cached.meta.context_limit ?? 0 };
      if (cached.meta.model) {
        tab.composer.model = cached.meta.model;
        comp = comp;
      }
      noteThreads([cached.meta]);
      if (tab.sessionId === id && cached.meta.title) patchTab(tab.id, { title: cached.meta.title });
      void scrollToBottom();
    }
    try {
      const [m, ev] = await api.getThread(id);
      if (my !== navSeq || activeId !== at) return;
      const changed = !cached || eventsDiffer(cached.events, ev);
      cacheThread(id, m, ev);
      meta = m;
      if (changed) showStored(id, ev);
      context = { used: m.context_tokens ?? 0, limit: m.context_limit ?? 0 };
      if (m.model) {
        tab.composer.model = m.model;
        comp = comp;
      }
      noteThreads([m]);
      if (tab.sessionId === id && m.title) patchTab(tab.id, { title: m.title });
      if (changed) await scrollToBottom();
    } catch (e) {
      if (my === navSeq) toastError(e);
    }
  }

  // A message just sent stays on screen until the store has it. The run
  // records it when it starts, which can come after the first reload (an agent
  // still starting, a queued run); without this the bubble vanished and only
  // "Thinking" showed.
  const usersIn = (ev: ChatEvent[]) => ev.filter((e) => e.kind === "user").length;
  function showStored(id: string, ev: ChatEvent[]) {
    act({ type: "stored", key: id, users: usersIn(ev) });
    stored = ev;
  }
  /** User messages the store had for a session when last read. */
  function usersOf(id: string): number {
    if (id === shown) return usersIn(stored);
    return usersIn(threadCache.get(id)?.events ?? []);
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
    cacheThread(id, m, ev);
    meta = m;
    showStored(id, ev);
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
    // Pushed last, so the tab the person closed is the first one Ctrl+Shift+T brings back.
    if (closed.kind === "session" && closed.sessionId) {
      const sessionId = closed.sessionId;
      closedTabs.update((list) => pushClosed(list, { url: "", title: closed.title, at: Date.now(), sessionId }));
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

  /** The folder of the session last worked on, leaving out throwaway scratch folders. */
  function recentFolder(): string | undefined {
    if (remoteOn) return undefined;
    const recent = threads
      .filter((t) => t.cwd && !/scratch/i.test(t.cwd) && !t.parent_id)
      .reduce<SessionMeta | null>((a, t) => (!a || t.updated > a.updated ? t : a), null);
    return recent?.cwd;
  }

  function newSession() {
    const draft: Tab = { ...sessionTab(), cwd: folder || recentFolder() };
    draft.composer = ensureComposer(draft.composer);
    tabs = placeTab(tabs, draft, activeId);
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
      if (background) tabs = placeTab(tabs, t, activeId);
      else addTab(t);
    }
  }

  // The side bar reads one file per session (tabs outside a session share one).
  let sessionDocs: Record<string, ArtifactDoc> = {};
  let looseDoc: ArtifactDoc | null = null;
  $: group = tab.kind === "session" ? (tab.sessionId ?? null) : (tab.group ?? null);
  $: fileDoc = group ? (sessionDocs[group] ?? null) : looseDoc;

  function closeFile() {
    if (!group) looseDoc = null;
    else if (group in sessionDocs) {
      const { [group]: _, ...rest } = sessionDocs;
      sessionDocs = rest;
    }
  }

  function putFile(doc: ArtifactDoc, at = group) {
    const prev = at ? sessionDocs[at] : looseDoc;
    // Reopening the same file bumps the version so the card re-renders.
    const version = prev?.id === doc.id ? Math.max(prev.version + 1, doc.version || 1) : doc.version || 1;
    const next = { ...doc, version };
    if (at) sessionDocs = { ...sessionDocs, [at]: next };
    else looseDoc = next;
    // Shown here: the side bar opens to read it.
    if (at === group && !infoOpen) toggleInfo(true);
  }

  /** An agent's ui.show_file: the file or brain note opens in that session's
   *  side bar, beside the cards. Errors go back to the agent rather than to
   *  a toast. */
  async function showFileFor(session: string, path: string, note: boolean): Promise<{ text: string }> {
    const at = session || group;
    if (note) {
      const content = await brain.read(path).catch(() => null);
      if (content == null) throw new Error(`There is no note ${path} in the brain, and no file of that name in the session's folder.`);
      const title = path.split("/").pop()?.replace(/\.md$/i, "") || path;
      putFile({ id: path, title, kind: "markdown", content, version: 1 }, at);
    } else {
      if (isImagePath(path)) putFile(imageDoc(path), at);
      else putFile(diskDoc(path, await api.readTextFile(path)), at);
    }
    return { text: `${path} is open beside the session${at === group ? "" : "; the person sees it when they open this session"}.` };
  }

  /** A linked file opens in the side bar; one that isn't workspace text opens outside Parzi. */
  async function openFileInCard(path: string) {
    const at = group;
    if (isImagePath(path)) {
      putFile(imageDoc(path), at);
      return;
    }
    let content: string;
    try {
      content = await api.readTextFile(path);
    } catch {
      await api.openFilePath(path).catch(toastError);
      return;
    }
    putFile(diskDoc(path, content), at);
  }

  function openArtifact(doc: ArtifactDoc) {
    if (doc.kind === "markdown" || doc.kind === "md") {
      putFile(doc);
      return;
    }
    const existing = tabs.find((t) => t.kind === "artifact" && t.doc?.id === doc.id && (t.group ?? null) === group);
    if (existing) {
      patchTab(existing.id, { doc, title: doc.title });
      selectTab(existing.id);
      return;
    }
    const t = artifactTab(doc, group);
    tabs = placeTab(tabs, t, activeId, group);
    selectTab(t.id);
  }

  /** A web page full screen over the current tab. A new link gets a fresh
   *  page rather than the last one's history. `id` is the one the desk
   *  gave a page the command line opened. */
  function openFullPage(url: string, id?: string) {
    if (!url) return;
    dropCoverPage();
    coverPage = pageTab(url, id);
    coverTarget = url;
    showCover("page");
  }

  /** A file full screen over the current tab, in the file reader; one that
   *  isn't text opens outside Parzi. */
  async function openFullFile(path: string) {
    let doc: ArtifactDoc;
    if (isImagePath(path)) doc = imageDoc(path);
    else {
      try {
        doc = diskDoc(path, await api.readTextFile(path));
      } catch {
        await api.openFilePath(path).catch(toastError);
        return;
      }
    }
    showFullDoc(doc, path);
  }

  async function openFullNote(path: string) {
    const content = await brain.read(path).catch(() => null);
    if (content == null) {
      toast("That note can't be read.", true);
      return;
    }
    const title = path.split("/").pop()?.replace(/\.md$/i, "") || path;
    showFullDoc({ id: path, title, kind: "markdown", content, version: 1 }, path);
  }

  function showFullDoc(doc: ArtifactDoc, target: string) {
    // Reopening the same file bumps the version so the reader re-renders.
    const version = coverDoc?.id === doc.id ? coverDoc.version + 1 : doc.version || 1;
    coverDoc = { ...doc, version };
    coverTarget = target;
    showCover("file");
  }

  function openPin(pin: Pin) {
    if (pin.kind === "url") openFullPage(pin.target);
    else void openFullFile(pin.target);
  }

  function openFromPage(opener: string, url: string) {
    const from = pageById(opener);
    // The page over a session keeps its place: what it opens in a new
    // window goes to the system browser.
    if (from && from === coverPage) {
      api.openConfirmedUrl(url).catch(toastError);
      return;
    }
    if (!from?.owner) {
      openFullPage(url);
      return;
    }
    const t = pageTab(url, undefined, from.owner);
    tabs = placeTab(tabs, t, opener);
    api.browserPrepare(t.id, url, from.owner).catch(() => {});
  }

  /** A page tab, or the page open over the thread. */
  function pageById(id: string): Tab | null {
    return coverPage?.id === id ? coverPage : (tabs.find((t) => t.id === id) ?? null);
  }

  function patchPage(id: string, patch: Partial<Tab>) {
    if (coverPage?.id === id) coverPage = { ...coverPage, ...patch };
    else patchTab(id, patch);
  }

  async function recoverPage(id: string) {
    try {
      await api.browserRecover(id);
    } catch (e) {
      toastError(e);
    }
    await new Promise((r) => setTimeout(r, 250));
    patchPage(id, { crashed: undefined, loading: false });
  }

  function navigatePage(id: string, url: string) {
    patchPage(id, { url, title: hostOf(url) });
    api.browserNavigate(id, url).catch(toastError);
  }

  const pendingTitles = new Map<string, string>();

  function onPage(e: PageEvent) {
    const t = pageById(e.tab);
    if (!t) return;
    const patch: Partial<Tab> = {};
    if (e.loading !== undefined) patch.loading = e.loading;
    if (e.canGoBack !== undefined) patch.canGoBack = e.canGoBack;
    if (e.canGoForward !== undefined) patch.canGoForward = e.canGoForward;
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
    patchPage(e.tab, patch);
    if (e.fullscreen !== undefined && (e.tab === activeId || (coverPageOn && e.tab === coverPage?.id))) void setImmersive(e.fullscreen);
  }

  function browse(url: string) {
    openFullPage(url);
  }

  function openSession(id: string) {
    const existing = tabs.find((t) => t.sessionId === id);
    if (existing) {
      selectTab(existing.id);
      return;
    }
    const title = displayTitle(threads.find((t) => t.id === id)?.title, "Session");
    const host = hostSessionOf(id);
    if (host) {
      const t = { ...sessionTab(id, title), group: host };
      t.composer = ensureComposer(t.composer);
      tabs = placeTab(tabs, t, activeId, host);
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

  /** The Environment card moved the session to another folder or project. */
  function relocated(m: SessionMeta) {
    if (m.id === shown) void reload();
    void refreshThreads();
  }

  function openSettings(section = "general") {
    settingsSection = section === "system" || section === "remote" || section === "skills" ? "general" : section;
    showCover("settings");
  }

  // The top-left buttons act as Back when their own page is already open.
  function toggleSettings() {
    if (coverOn && cover === "settings") clearCover();
    else openSettings();
  }

  function toggleManager() {
    if (coverOn && cover === "sessions") clearCover();
    else showCover("sessions");
  }

  /** Answers the session's proposed plan; false when that failed. */
  async function settleProposal(session: string, accept: boolean): Promise<boolean> {
    try {
      const m = await api.answerPlanProposal(session, accept);
      if (meta?.id === session) meta = m;
      threads = threads.map((t) => (t.id === session ? { ...t, proposal: null } : t));
      return true;
    } catch (e) {
      toastError(e);
      return false;
    }
  }

  /** Accept on a proposed plan: the answer goes to the agent as the
   *  person's next message (its turn ended when it proposed), from the tab
   *  it was given in, and whatever they were typing stays in the composer. */
  let accepting = "";
  async function answerProposal(session: string) {
    const target = tab;
    if (target.sessionId !== session || accepting === session) return;
    accepting = session;
    try {
      if (await settleProposal(session, true)) await sendText(target, "Use this plan.");
    } finally {
      accepting = "";
    }
  }

  /** Sends given words (and files) from a tab as the next message, leaving
   *  the draft in its composer as it is. */
  function sendText(target: Tab, text: string, files: string[] = []) {
    return send({ target, text, files });
  }

  /** Sends the composer's draft, or given words from a tab (sendText). */
  async function send(given?: { target: Tab; text: string; files: string[] }) {
    const target = given?.target ?? tab;
    target.composer = ensureComposer(target.composer);
    const draft = target.composer;
    const prompt = (given ? given.text : draft.input).trim();
    if (!prompt) return;
    /** Empties the composer, when it was the draft that went. */
    const clearDraft = () => {
      if (given) return;
      draft.input = "";
      draft.attachments = [];
      comp = comp;
    };
    if (!draft.model && draft.mode !== "search") {
      toast("Pick a model first");
      omnibar?.openModels();
      return;
    }
    if (isExplicitUrl(prompt)) {
      clearDraft();
      browse(toAddress(prompt));
      return;
    }
    // Words sent while a plan waits answer it: the card goes, the words go
    // to the agent as usual. Read from the session itself, not from what
    // the window last drew.
    const plans = target.sessionId && metaOf(target.sessionId)?.proposal?.items?.length ? target.sessionId : "";
    if (plans && !(await settleProposal(plans, false))) return;
    const files = given ? given.files : [...draft.attachments];
    const place = placeOf(target);
    const agentOf = (model: string) => model.split("/")[0];
    const sessionModel = threads.find((t) => t.id === target.sessionId)?.model ?? "";
    // Another agent takes the same thread over (parzi-runtime route): the
    // session and its history stay, the new agent is handed the conversation.
    const otherAgent = !!sessionModel && !!draft.model && agentOf(sessionModel) !== agentOf(draft.model);
    const fresh = !target.sessionId;
    const where = place.cwd;
    const key = target.sessionId ?? tabKey(target.id);
    const item = { tab: target.id, text: prompt, files, model: draft.model, effort: draft.effort, mode: permission, cwd: where, remote: place.remote };
    if (isBusy(sends, key)) {
      // A new tab's first message is still on its way: there is nothing to queue behind yet.
      if (fresh) return;
      if (!act({ type: "hold", key, item }).ok) {
        toast(`The queue holds ${MAX_HELD} messages`);
        return;
      }
      clearDraft();
      return;
    }
    const wasDraft = threads.find((t) => t.id === target.sessionId)?.status === "draft";
    if (target.sessionId && otherAgent && !wasDraft) toast(`${nameOf(agentOf(draft.model))} takes over this thread, with the conversation so far`);
    act({ type: "begin", key, text: prompt, users: fresh ? 0 : usersOf(key) });
    if (!given) forgetDraft(target);
    clearDraft();
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
        remote: place.remote,
        worktree: !target.sessionId && !place.remote ? draft.worktree : undefined,
      });
      act({ type: "started", key, sid });
      rememberModel(draft.model, draft.effort);
      clearDraft();
      if (fresh) patchTab(target.id, { sessionId: sid, title: (prompt || "New session").slice(0, 40) });
      else if (sid !== target.sessionId) patchTab(target.id, { sessionId: sid });
      if (activeId === target.id) {
        shown = sid;
        await reload();
      }
      void refreshThreads();
    } catch (e) {
      // The engine is still letting go of the last run: the message waits in the queue.
      const still = !fresh && stillWorking(e);
      if (act({ type: "failed", key, item: still ? item : undefined, stillWorking: still }).ok && still) {
        console.info("queue: still working; the message stays queued", { session: key });
        return;
      }
      // What failed goes back to the composer, unless the person is
      // already typing something else there.
      if (!given || (!draft.input.trim() && !draft.attachments.length)) {
        draft.input = prompt;
        draft.attachments = files;
        comp = comp;
      }
      if (still) toast(`The queue holds ${MAX_HELD} messages`);
      else toastError(e);
    }
  }

  /** What the window knows of a session: the one on screen, or the list's. */
  function metaOf(sid: string): SessionMeta | null {
    return (meta?.id === sid ? meta : threads.find((x) => x.id === sid)) ?? null;
  }

  /** Where a message from this tab runs: the remote or this PC, and in
   *  which folder (a session's own, or the one picked for a new one). */
  function placeOf(t: Tab): { remote: boolean; cwd: string } {
    const sid = t.sessionId;
    const remote = sid ? isRemote(sid) : !!$remoteInfo && (t.composer.remote ?? preferRemote);
    if (sid) return { remote, cwd: metaOf(sid)?.cwd ?? "" };
    return { remote, cwd: remote ? (t.composer.remoteCwd ?? "") : (t.cwd ?? "") };
  }

  function restore(sid: string, h: HeldItem) {
    const t = tabs.find((x) => x.id === h.tab) ?? tabs.find((x) => x.sessionId === sid);
    if (!t) return;
    t.composer = ensureComposer(t.composer);
    const c = t.composer;
    c.input = [h.text, c.input.trim()].filter(Boolean).join("\n\n");
    c.attachments = [...new Set([...h.files, ...c.attachments])];
    if (t.id === activeId) {
      syncComposer(t);
      comp = comp;
    }
  }

  function unhold(sid: string, itemId?: string) {
    const { item } = act({ type: "take", key: sid, itemId });
    if (item) restore(sid, item);
  }

  /** Hands a queued message to the engine; the send state already counts it as sending. */
  async function sendHeld(sid: string, h: HeldItem) {
    if (sid === shown) await reload();
    act({ type: "echo", key: sid, text: h.text, users: usersOf(sid) });
    if (sid === shown) void scrollToBottom(true);
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
      });
      act({ type: "started", key: sid, sid: id });
      if (id !== sid) {
        for (const t of tabs.filter((t) => t.sessionId === sid)) patchTab(t.id, { sessionId: id });
      }
      if (shown === sid) {
        shown = id;
        await reload();
      }
      void refreshThreads();
    } catch (e) {
      const still = stillWorking(e);
      act({ type: "failed", key: sid, item: h, front: true, stillWorking: still });
      // The previous run may still be releasing its handle: the send state
      // keeps it working and tries again shortly.
      if (still) console.info("queue: still working; scheduling retry", { session: sid });
      else toastError(e);
    }
  }

  async function stop() {
    if (!shown || !isWorking(sends, shown)) return;
    const id = shown;
    act({ type: "stop", key: id });
    clearLive();
    await reload();
    void refreshThreads();
  }

  function dropQueued(sid: string, itemId: string) {
    act({ type: "drop", key: sid, itemId });
  }

  function moveQueued(sid: string, itemId: string, to: number) {
    act({ type: "move", key: sid, itemId, to });
  }

  function resumeQueue(sid: string) {
    act({ type: "pause", key: sid, on: false });
  }

  /** Stops the agent if it is at work and sends this queued message once it has let go. */
  function sendHeldNow(itemId?: string) {
    if (!shown) return;
    const r = act({ type: "sendNow", key: shown, itemId });
    if (r.effects.some((fx) => fx.do === "kill")) clearLive();
  }

  /** Carries out a stop: the engine's kill returns once the run is gone. */
  async function stopSession(id: string) {
    try {
      await api.killRun(id);
    } catch (e) {
      toastError(e);
    }
    act({ type: "stopped", key: id });
    void refreshThreads();
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
    let files: string[] = [];
    let taken: HeldItem | undefined;
    if (fromHeld) {
      taken = act({ type: "take", key: parent, itemId }).item;
      if (!taken) return;
      ({ text, files } = taken);
    } else {
      text = comp.input.trim();
      files = [...comp.attachments];
      if (!text) return;
      comp.input = "";
      comp.attachments = [];
      comp = comp;
    }
    const giveBack = () => {
      if (taken) act({ type: "putBack", key: parent, item: taken });
      else {
        comp.input = [text, comp.input].filter(Boolean).join("\n\n");
        comp.attachments = [...files, ...comp.attachments];
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
      await launchSide(parent, text, files);
    } catch (e) {
      giveBack();
      toastError(e);
    }
  }

  async function launchSide(parent: string, text: string, files: string[]): Promise<string> {
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
      parent,
    });
    act({ type: "started", key: id, sid: id });
    void refreshThreads();
    const busySame = !aimed.folder && isWorking(sends, parent);
    toast(busySame ? "Side session started in this folder too: the two may edit the same files" : `Side session started${aimed.title ? ` in ${aimed.title}` : ""}`);
    return id;
  }

  async function sendReply(text: string) {
    if (sending || streaming) return;
    await sendText(tab, text);
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

  async function rewindTo(nth: number, text: string) {
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

  // "Change model" on a failed turn (a usage limit, say): the model picked
  // next resends the last message on the same thread, so nothing is retyped.
  let retryPick: { tab: string; from: string } | null = null;
  function pickAndRetry() {
    retryPick = { tab: activeId, from: comp.model };
    omnibar?.openModels();
  }
  $: if (retryPick && (retryPick.tab !== activeId || streaming)) retryPick = null;
  $: if (retryPick && comp.model && comp.model !== retryPick.from) {
    retryPick = null;
    retryLast();
  }

  // Start on a draft: its brief goes as the first message, on the model in
  // the composer; the plan it carries stays on the session.
  function startDraft() {
    if (!meta || meta.status !== "draft" || sending) return;
    if (meta.brief?.trim()) void sendText(tab, meta.brief);
    else void send();
  }

  function retryLast() {
    const last = [...events].reverse().find((e) => e.kind === "user");
    if (!last || last.kind !== "user" || streaming || sending) return;
    // Resend the files the message carried, not just its words.
    const text = last.text
      .replace(/\n?\[attached: [^\]]+\]/, "")
      .replace(/\n*<side-task[^>]*>[\s\S]*?<\/side-task>\n*/, "")
      .trimEnd();
    void sendText(tab, text, attachedPaths(last.text));
  }

  async function remove(id: string) {
    const title = threads.find((t) => t.id === id)?.title || meta?.title || "this session";
    const ok = await ask(`Delete "${title}" and its transcript?`, { title: "Delete session", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await api.deleteThread(id);
      act({ type: "deleted", key: id });
      if (sessionDocs[id]) {
        const { [id]: _, ...rest } = sessionDocs;
        sessionDocs = rest;
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
    const idle = threads.filter((t) => !isWorking(sends, t.id));
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
      const kept = new Set(threads.map((t) => t.id));
      for (const t of idle.filter((t) => !kept.has(t.id))) act({ type: "deleted", key: t.id });
      for (const t of tabs.filter((t) => t.sessionId && !kept.has(t.sessionId))) closeTab(t.id);
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
    else if (name === "settings") openSettings();
  }

  // Text arrives in bursts; it is shown at an even pace measured in time,
  // not frames: at least 600 characters a second, faster when behind so the
  // backlog clears in about a tenth of a second. Same speed at 60 or 120Hz.
  let revealAt = 0;
  let thinking = false;
  function revealLive(now: number) {
    revealing = 0;
    if (!pending) return;
    const dt = Math.min(64, revealAt ? now - revealAt : 16);
    revealAt = now;
    let cut = Math.min(pending.length, Math.max(1, Math.ceil(dt * 0.6), Math.ceil((pending.length * dt) / 100)));
    if (cut < pending.length && /[\uD800-\uDBFF]/.test(pending[cut - 1])) cut++;
    live += pending.slice(0, cut);
    pending = pending.slice(cut);
    if (pending) revealing = requestAnimationFrame(revealLive);
    else revealAt = 0;
  }

  const titleOf = (session: string) => threads.find((t) => t.id === session)?.title || "Agent";

  /** Tells the person about an agent they are not looking at:
   *  - "needs": an approval, question or request. A toast that goes once
   *    it is answered, and a short tone if they are elsewhere.
   *  - "done": a run finished. Only its tab says so.
   *  - "problem": an error or a spending pause. A toast.
   *  A subagent's finish or error is news for the agent that started it,
   *  not the person. A toast never shows while the window is in the
   *  background: it would be gone before they look, and the tab keeps it. */
  function announce(session: string, what: string, opts: { key?: string; news: "needs" | "done" | "problem" }) {
    const focused = document.hasFocus();
    if (session === shown && tab.kind === "session" && focused) return;
    const child = threads.some((t) => t.id === session && t.parent_id);
    if (child && opts.news !== "needs") return;
    if (opts.news === "done") return;
    const key = opts.key ?? `news:${session}`;
    // The side column lists what is waiting; a toast would say it twice.
    if (focused && !infoShown) keyedToast(key, `${titleOf(session)} ${what}`, { session, ms: opts.news === "needs" ? 10000 : 6000 });
    if (opts.news === "needs") chime(false);
  }

  /** Marks a run's end on its tab until the session is opened. */
  function markEnded(session: string, mark: SessionMark) {
    if (session === shown && tab.kind === "session") return;
    unseen = { ...unseen, [session]: mark };
  }

  function settle(key: string) {
    keyedToast(key, null);
    approvals = approvals.filter((a) => a.key !== key);
    questions = questions.filter((q) => q.key !== key);
    requests = requests.filter((r) => r.key !== key);
  }

  const stopped = (error: string) => {
    const t = (error ?? "").trim().toLowerCase();
    return !!t && t.length <= 100 && (t.includes("cancelled") || t.includes("canceled"));
  };

  function onEvent(e: UiEvent) {
    if (e.kind === "approval") {
      const fresh = !approvals.some((a) => a.key === e.key);
      approvals = [...approvals.filter((a) => a.key !== e.key), { key: e.key, session: e.session, call: e.call }];
      if (fresh) announce(e.session, `needs your approval: ${e.call?.label || e.call?.name || "an action"}`, { key: e.key, news: "needs" });
      return;
    }
    if (e.kind === "question") {
      const fresh = !questions.some((q) => q.key === e.key);
      questions = [...questions.filter((q) => q.key !== e.key), { key: e.key, session: e.session, question: e.question, options: e.options, details: e.details, multi: e.multi }];
      if (fresh) announce(e.session, `asks: ${e.question || "a question"}`, { key: e.key, news: "needs" });
      return;
    }
    if (e.kind === "request") {
      const fresh = !requests.some((r) => r.key === e.key);
      requests = [...requests.filter((r) => r.key !== e.key), { key: e.key, session: e.session, request: e.request, kind: e.req_kind }];
      if (fresh) announce(e.session, `needs something from you: ${e.request || "a request"}`, { key: e.key, news: "needs" });
      return;
    }
    if (e.kind === "expired") {
      settle(e.key);
      return;
    }
    if (e.kind === "done" || e.kind === "error" || e.kind === "paused") {
      if (e.session !== shown) dropCachedThread(e.session);
      for (const k of [...approvals, ...questions, ...requests].filter((x) => x.session === e.session).map((x) => x.key)) settle(k);
      void refreshThreads();
      const outcome: Outcome =
        e.kind === "done" ? "done" : e.kind === "paused" ? "paused" : stopped(e.error) ? "stopped" : /exited before/i.test(e.error) ? "exited" : "failed";
      const was = isWorking(sends, e.session);
      // A finished run sends the next queued message (sendHeld reloads first).
      const next = act({ type: "ended", key: e.session, outcome }).effects.some((fx) => fx.do === "deliver");
      if (e.session === shown) {
        if (next) clearLive();
        else {
          // The turn on screen ends once its saved message is in place: ending
          // it first drops the working line and the live fold while the reply
          // still shows, then the saved message pops in.
          const id = e.session;
          if (was) closing = id;
          void reload(clearLive).finally(() => {
            if (closing === id) closing = "";
          });
        }
      }
      if (e.kind === "paused") {
        announce(e.session, "paused at its spending limit", { news: "problem" });
        markEnded(e.session, "waiting");
      } else if (e.kind === "done") {
        announce(e.session, "finished", { news: "done" });
        markEnded(e.session, "done");
      } else if (!stopped(e.error)) {
        // A stop someone asked for (the person, or a parent agent) is not an error.
        announce(e.session, "stopped with an error", { news: "problem" });
        markEnded(e.session, "failed");
      }
      return;
    }
    // The working agent formatted a message for display (session.format_message).
    if (e.kind === "refreshed") {
      if (e.session !== shown) dropCachedThread(e.session);
      void renamed(e.session);
      if (e.session === shown) void reload();
      return;
    }
    if (e.kind !== "context") act({ type: "activity", key: e.session });
    if (e.kind === "tool_result" && e.name === "session.rename" && e.ok) void renamed(e.session);
    if (e.kind === "tool_result" && e.name === "session.format_message" && e.ok && e.session === shown) void reload();
    if (e.kind === "tool_result" && (e.name === "project.switch" || e.name === "project.create") && e.ok) void moved(e.session);
    if (e.session !== shown) return;
    if (e.kind === "text") {
      thinking = false;
      pending += e.text;
      if (!revealing) revealing = requestAnimationFrame(revealLive);
    } else if (e.kind === "reasoning") {
      // A new spell of thinking after a step or after narration starts its
      // own paragraph instead of running on from the last sentence.
      if (!thinking && liveReasoning) liveReasoning += "\n\n";
      thinking = true;
      liveReasoning += e.text;
    } else if (e.kind === "todo") {
      liveTodo = e.todo;
    } else if (e.kind === "artifact") {
      liveArtifacts = [...liveArtifacts.filter((a) => a.id !== e.id), e];
    } else if (e.kind === "tool_call") {
      thinking = false;
      // What the agent said before a step is narration ("I'll check the
      // plan..."), not its reply: it joins the folded working, and the reply
      // starts again after the step. Cards and reply buttons come after the
      // final text, so they leave it be.
      const said = (live + pending).trim();
      const keeps = e.name.startsWith("ui.") || /^todo[._]?write$/i.test(e.name);
      if (said && !keeps) {
        liveReasoning = liveReasoning ? `${liveReasoning}\n\n${said}` : said;
        thinking = false;
        live = "";
        pending = "";
      } else if (said) {
        // The reply goes on after the card; the next words start a new
        // paragraph instead of running on from the last one.
        pending += "\n\n";
        if (!revealing) revealing = requestAnimationFrame(revealLive);
      }
      if (!liveTools.some((t) => t.id === e.id)) {
        liveTools = [...liveTools, { id: e.id, name: e.name, label: e.label || e.name, running: true, ok: true, ms: 0 }];
      }
    } else if (e.kind === "tool_result") {
      liveTools = liveTools.map((t) => (t.running && t.id === e.id ? { ...t, running: false, ok: e.ok, ms: e.ms } : t));
    } else if (e.kind === "context") {
      context = { used: e.used, limit: e.limit };
    } else if (e.kind === "notice") {
      if (freshNotice(e.session, e.text)) toast(e.text);
    }
  }

  function shortcut(name: string): boolean {
    if (name === "ctrl+p" || name === "ctrl+k") toggleManager();
    else if (name === "ctrl+t") newSession();
    else if (name === "ctrl+w") {
      closeTab(activeId);
    }
    else if (name === "ctrl+comma") toggleSettings();
    else if (name === "ctrl+j" || name === "ctrl+shift+a") toggleInfo();
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
              : mod && ["p", "k", "t", "w", "l", "j"].includes(key)
                  ? `ctrl+${key}`
                  : "";
    if (name && shortcut(name)) {
      e.preventDefault();
    } else if (key === "escape" && !e.defaultPrevented && !$covered) {
      if (immersive) void setImmersive(false);
      else if (coverOn) clearCover();
    }
  }

  function onDeskCmd(cmd: DeskCmd) {
    // An agent's own page (browser.open) stays a tab it drives in the
    // background; any other page opens over the current tab.
    if (cmd.op === "open" && cmd.url) {
      if (cmd.owner) openPage(cmd.url, cmd.id, cmd.owner, !cmd.focus);
      else openFullPage(cmd.url, cmd.id);
    }
    else if (cmd.op === "navigate" && cmd.id && cmd.url) patchPage(cmd.id, { url: cmd.url, title: hostOf(cmd.url) });
    else if (cmd.op === "focus" && cmd.id) {
      if (coverPage?.id === cmd.id) showCover("page");
      else selectTab(cmd.id);
    }
    else if (cmd.op === "close" && cmd.id) {
      if (coverPage?.id === cmd.id) clearCover();
      else closeTab(cmd.id);
    }
    else if (cmd.op === "show") showFromDesk(cmd);
    else if (cmd.op === "call" && cmd.id) void answerCall(cmd.id, cmd.call ?? {});
    else if (cmd.op === "theme") {
      // An agent changed the look: load it, and the wallpaper a pack may bring.
      void api
        .getThemeCss()
        .then(applyThemeCss)
        .then(() => refreshBackground())
        .then(() => document.dispatchEvent(new CustomEvent("parzi:theme")))
        .catch((e) => toast(`Couldn't load the new theme: ${e}`, true));
    }
    else if (cmd.op === "notify" && cmd.text) {
      // An agent's own heads-up: at most one a minute per session, and no
      // tone while the person is looking at that session.
      const sid = cmd.session ?? "";
      if (freshNotice(`notify:${sid}`, "", Date.now(), 60_000)) {
        notify(cmd.text, sid);
        const watching = document.hasFocus() && sid === shown && tab.kind === "session";
        if (!watching) chime(true);
      }
    } else if (cmd.op === "approval" && cmd.key) {
      approvals = approvals.filter((a) => a.key !== cmd.key);
      if (cmd.label) toast(`${cmd.allow ? "Allowed" : "Denied"} from the command line: ${cmd.label}`);
    } else if (cmd.op === "deleted" && cmd.id) {
      const gone = cmd.id;
      dropCachedThread(gone);
      act({ type: "deleted", key: gone });
      for (const t of tabs.filter((x) => x.sessionId === gone)) closeTab(t.id);
      void refreshThreads();
    }
  }

  /** Where the window is, in a line, for an agent looking at it. */
  function whereNow(): string {
    const covers = { settings: `Settings, ${settingsSection}`, sessions: "Sessions", page: `the page ${coverPage?.url ?? ""}`, file: `the file ${coverDoc?.title ?? ""}` };
    const page = coverOn && cover ? covers[cover] : "";
    const kind = tab.kind === "page" ? "web page" : tab.kind === "session" ? (hasSession ? "session" : "Home") : tab.kind;
    return `Window: ${kind} tab "${tab.title || "New session"}"${page ? `, showing ${page}` : ""}${infoShown ? ", side column open" : ""}${readerShown && fileDoc ? `, reading ${fileDoc.title}` : ""}.`;
  }

  /** Answers the desk: an agent listing or running Parzi's actions, or
   *  looking at and using its controls (control.rs app_call). */
  async function answerCall(id: string, call: Record<string, unknown>) {
    const n = (k: string) => Number(call[k]);
    const str = (k: string) => (typeof call[k] === "string" ? (call[k] as string) : "");
    let result: unknown;
    try {
      switch (call.kind) {
        case "actions":
          result = { text: `${whereNow()}\n${describeActions(str("query"))}` };
          break;
        case "do":
          result = await runAction(str("action"), (call.args as Record<string, unknown>) ?? {}, call.confirmed === true);
          await tick();
          break;
        case "screen":
          result = { text: readScreen(str("query"), whereNow()) };
          break;
        case "click":
          result = clickControl(n("ref"), call.confirmed === true);
          await tick();
          break;
        case "type":
          result = typeControl(n("ref"), str("text"), call.enter === true);
          break;
        case "show_file":
          result = await showFileFor(str("session"), str("path"), call.note === true);
          break;
        default:
          result = { ok: false, error: `unknown call ${String(call.kind)}` };
      }
    } catch (e) {
      result = { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
    await deskAnswer(id, result).catch(() => {});
  }

  function tabNamed(raw: unknown): Tab | undefined {
    const want = String(raw ?? "").trim().toLowerCase();
    if (!want) return tab;
    return tabs.find((t) => t.id === want || t.sessionId === want) ?? tabs.find((t) => (t.title || "").toLowerCase() === want) ?? tabs.find((t) => (t.title || "").toLowerCase().includes(want));
  }

  /** What Parzi can do by name. A surface that is not here an agent still
   *  reaches with app_screen and app_click. */
  function appActions(): AppAction[] {
    const onOff = (v: unknown, now: boolean) => (typeof v === "boolean" ? v : !now);
    const theme = async (change: (t: Awaited<ReturnType<typeof api.getTheme>>) => void) => {
      const t = await api.getTheme();
      change(t);
      await api.saveTheme(t);
      applyThemeCss(await api.getThemeCss());
      document.dispatchEvent(new CustomEvent("parzi:theme"));
    };
    return [
      { id: "tabs.list", title: "List the open tabs", run: () => tabs.map((t) => `${t.id === activeId ? "* " : ""}${t.kind} "${t.title || "New session"}" (${t.id})${t.url ? ` ${t.url}` : ""}`).join("\n") },
      { id: "tabs.new", title: "Open a new session tab (Home)", run: () => void newSession() },
      { id: "page.open", title: "Open a web page full screen over the current tab", args: [{ name: "url", type: "string", required: true, about: "an address or words to search" }], run: (a) => { const url = toAddress(String(a.url ?? "")); if (!url) throw new Error("Say which page to open."); openFullPage(url); return `Opened ${url}.`; } },
      { id: "tabs.select", title: "Switch to a tab by title or id", args: [{ name: "tab", type: "string", required: true }], run: (a) => { const t = tabNamed(a.tab); if (!t) throw new Error(`No tab "${a.tab}".`); selectTab(t.id); return `Switched to "${t.title || "New session"}".`; } },
      { id: "tabs.close", title: "Close a tab (the current one if none is named)", args: [{ name: "tab", type: "string" }], run: (a) => { const t = tabNamed(a.tab); if (!t) throw new Error(`No tab "${a.tab}".`); closeTab(t.id); return `Closed "${t.title || "the tab"}".`; } },
      { id: "tabs.reopen", title: "Reopen the last closed tab", run: () => void reopenClosed() },
      { id: "sessions.show", title: "Show the list of sessions", run: () => void showCover("sessions") },
      { id: "sessions.open", title: "Open a session by id or title", args: [{ name: "session", type: "string", required: true }], run: (a) => { const want = String(a.session).toLowerCase(); const hit = threads.find((t) => t.id === a.session || t.id.startsWith(want) || t.title.toLowerCase() === want) ?? threads.find((t) => t.title.toLowerCase().includes(want)); if (!hit) throw new Error(`No session "${a.session}".`); openSession(hit.id); return `Opened "${hit.title}".`; } },
      { id: "session.stop", title: "Stop the current session's run", when: () => hasSession, run: async () => { await stop(); return "Stopped."; } },
      { id: "session.fork", title: "Branch the current session into a copy", when: () => hasSession, run: async () => { await fork(); return "Forked the session."; } },
      { id: "session.compact", title: "Summarise the current session to free context", when: () => hasSession, run: async () => { await compact(); return "Compacting the session."; } },
      { id: "settings.open", title: "Open Settings at a page", args: [{ name: "section", type: "string", choices: ["general", "providers", "connections", "appearance"] }], run: (a) => { openSettings(String(a.section ?? "general")); return `Opened Settings, ${a.section ?? "general"}. Use app_screen to see its controls.`; } },
      { id: "settings.close", title: "Close Settings, the sessions list, or the page or file open over the session", when: () => coverOn, run: () => void clearCover() },
      { id: "sidebar.toggle", title: "Show or hide the session's side column", args: [{ name: "open", type: "boolean" }], run: (a) => { toggleInfo(onOff(a.open, infoShown)); return infoOpen ? "The side column is open." : "The side column is hidden."; } },
      { id: "files.open", title: "Open a file in the side bar, beside the cards", args: [{ name: "path", type: "string", required: true }], run: async (a) => { await openFileInCard(String(a.path)); return `Opened ${a.path}.`; } },
      { id: "files.close", title: "Close the file open in the side bar", run: () => { closeFile(); return "Closed the file."; } },
      { id: "window.fullscreen", title: "Turn full screen on or off", args: [{ name: "on", type: "boolean" }], run: async (a) => { await setImmersive(onOff(a.on, immersive)); return immersive ? "Full screen is on." : "Full screen is off."; } },
      { id: "composer.model", title: "Pick the model for the next message", args: [{ name: "model", type: "string", required: true, choices: () => $board.flatMap((b) => b.models.map((m) => `${b.provider}/${m.id}`)) }], run: (a) => { comp.model = String(a.model); comp = comp; return `The next message goes to ${a.model}.`; } },
      { id: "composer.effort", title: "Set the reasoning effort", args: [{ name: "effort", type: "string", required: true, choices: () => effortsFor(comp.model, $board) }], run: (a) => { comp.effort = String(a.effort); comp = comp; return `Effort is ${a.effort}.`; } },
      { id: "composer.permission", title: "Set what agents may do without asking", args: [{ name: "mode", type: "string", required: true, choices: ["supervised", "auto", "full"] }], run: (a) => { permission = String(a.mode); return `Permission is ${a.mode}.`; } },
      { id: "composer.mode", title: "Switch the composer between the agent and web search", args: [{ name: "mode", type: "string", required: true, choices: ["agent", "web"] }], run: (a) => { comp.mode = a.mode === "web" ? "search" : "build"; comp = comp; return `The composer is in ${a.mode} mode.`; } },
      { id: "composer.draft", title: "Write text into the composer for the person to send", args: [{ name: "text", type: "string", required: true }], run: (a) => { comp.input = String(a.text); comp = comp; return "The text is in the composer; the person sends it."; } },
      { id: "appearance.theme", title: "Apply a theme pack", args: [{ name: "pack", type: "string", required: true }], run: async (a) => { const packs = await api.listPackInfos(); const p = packs.find((x) => x.name.toLowerCase() === String(a.pack).toLowerCase()); if (!p) throw new Error(`No theme "${a.pack}"; the themes are ${packs.map((x) => x.name).join(", ")}.`); applyThemeCss(await api.applyPack(p.name)); await refreshBackground(); document.dispatchEvent(new CustomEvent("parzi:theme")); return `Applied the ${p.name} theme.`; } },
      { id: "appearance.wallpaper", title: "Show the Home wallpaper as a band or the full window", args: [{ name: "show", type: "string", required: true, choices: ["band", "full"] }], run: async (a) => { await theme((t) => (t.background.fill = a.show === "full" ? "full" : "band")); return `The wallpaper shows as ${a.show === "full" ? "the full window" : "a band"}.`; } },
    ];
  }

  function showFromDesk(cmd: DeskCmd) {
    if (cmd.what === "session" && cmd.id) {
      const id = cmd.id;
      if (cmd.focus !== false) openSession(id);
      else if (!tabs.some((t) => t.sessionId === id)) {
        const t = sessionTab(id, displayTitle(cmd.title, "Session"));
        tabs = placeTab(tabs, t, activeId);
      }
    } else if (cmd.what === "settings") openSettings(cmd.section || "general");
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
    const unActivity = watchActivity();
    const unActions = defineActions(appActions());
    selectTab(activeId);
    if (!$onboarded) setupOpen = true;
    void warmPages();
    const onBg = (e: Event) => (bg = (e as CustomEvent<string>).detail);
    document.addEventListener("parzi:bg", onBg);
    const onOpenPage = (e: Event) => {
      const url = (e as CustomEvent<string>).detail ?? "";
      if (!url) return;
      openFullPage(url);
    };
    document.addEventListener("parzi:open-page", onOpenPage);
    const onOpenArtifact = (e: Event) => {
      const doc = (e as CustomEvent<ArtifactDoc>).detail;
      if (doc?.content) openArtifact(doc);
    };
    document.addEventListener("parzi:open-artifact", onOpenArtifact);
    const onOpenNote = (e: Event) => {
      const path = (e as CustomEvent<string>).detail ?? "";
      if (path) void openFullNote(path);
    };
    document.addEventListener("parzi:open-note", onOpenNote);
    const onOpenFile = (e: Event) => {
      const path = (e as CustomEvent<string>).detail ?? "";
      if (path) void openFullFile(path);
    };
    document.addEventListener("parzi:open-file", onOpenFile);
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
    let remoteDown = false;
    const unRemote = onRemoteStatus((s) => {
      if (s.state === "updating" || s.state === "ready") toast(s.detail);
      // Sync retries every two minutes and after each run: say a failure
      // once, until the remote comes back.
      else if (s.state === "error") {
        if (!remoteDown) toast(s.detail, true);
        remoteDown = true;
      }
      if (s.state === "up" || s.state === "synced" || s.state === "ready") remoteDown = false;
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
    // Each load stands alone: one failing must not leave the others undone.
    void api
      .getThemeCss()
      .then(applyThemeCss)
      .catch((e) => toast(`Couldn't load the theme: ${e}`, true));
    void api
      .backgroundUrl()
      .then((url) => (bg = url))
      .catch((e) => toast(`Couldn't load the wallpaper: ${e}`, true));
    void ensureBoard().catch((e) => toast(`Couldn't load the agents: ${e}`, true));
    checkForUpdatesSoon();
    return () => {
      unActivity();
      unActions();
      unClear();
      document.removeEventListener("parzi:open-page", onOpenPage);
      document.removeEventListener("parzi:open-artifact", onOpenArtifact);
      document.removeEventListener("parzi:open-note", onOpenNote);
      document.removeEventListener("parzi:open-file", onOpenFile);
      unRun.then((f) => f()).catch(() => {});
      unDesk.then((f) => f()).catch(() => {});
      unResync.then((f) => f()).catch(() => {});
      unRemote.then((f) => f()).catch(() => {});
      for (const un of [unPage, unOpen, unKey, unAsk]) un.then((f) => f()).catch(() => {});
      document.removeEventListener("parzi:bg", onBg);
    };
  });
</script>

<svelte:window on:keydown={onKey} on:pagehide={persistDrafts} on:beforeunload={persistDrafts} bind:innerWidth />

<div class="shell" class:on-home={onHome} class:over-art={overArt} class:art-shift={artShift}>
  <!-- Home shows the picture; sessions and covers keep it, dimmed. -->
  <DefaultArt bg={tab.kind === "session" ? bg : ""} dim={!showWallpaper} />

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
      on:manager={toggleManager}
      on:settings={toggleSettings}
      panelable={tab.kind === "session" || tab.kind === "page"}
      panelOpen={infoShown || pageInfoShown}
      panelCount={familyActive}
      on:panel={() => toggleInfo()}
    />
  {/if}

  <div class="content">
    <main>
    {#if tab.kind === "artifact" && tab.doc}
      <div class="fill col ghost" in:fade={{ duration: 150 }}>
        <PanelHeader title={tab.title} on:close={() => closeTab(activeId)} />
        {#key `${tab.id}:${tab.doc.version}`}
          <ArtifactCard data={tab.doc} reader />
        {/key}
      </div>
    {:else if tab.kind === "page"}
      <div class="stagerow">
        <div class="page-card">
          {#key tab.id}
            <PageView
              bind:this={page}
              {tab}
              {immersive}
              on:navigate={(e) => navigatePage(tab.id, e.detail.url)}
              on:error={(e) => toast(e.detail.text, true)}
              on:openTab={(e) => navigatePage(tab.id, e.detail.url)}
              on:recover={() => recoverPage(tab.id)}
              on:resume={() => patchTab(tab.id, { crashed: undefined })}
            />
          {/key}
        </div>
        {#if readerShown && fileDoc}
          <FileReader doc={fileDoc} folder={sideMeta?.cwd ?? folder} width={readerWidth} on:close={closeFile} on:resize={resizeFile} />
        {/if}
        <SessionInfo
          session={sideId}
          project={sessionProject}
          {projectTitle}
          folder={sideMeta?.cwd ?? ""}
          branch={sideMeta?.branch ?? ""}
          remoteHost=""
          contextUsed={0}
          contextLimit={0}
          {compacting}
          streaming={sideId ? working.has(sideId) : false}
          {threads}
          running={working}
          {approvals}
          questions={asks}
          width={infoWidth}
          open={pageInfoShown}
          on:resize={resizeInfo}
          tokensIn={0}
          tokensOut={0}
          costUsd={0}
          provider={sideMeta?.model?.split("/")[0] ?? ""}
          on:openSession={(e) => openSession(e.detail.id)}
          on:compact={compact}
          on:moved={(e) => relocated(e.detail.meta)}
        />
      </div>
    {:else}
      {#if tab.kind === "session" && (hasSession || infoShown || coverOn)}
        <div class="stagerow" class:solo={!hasSession && !coverOn} out:fade|local={{ duration: motion.duration }}>
          {#if hasSession || coverOn}
          <section class="session" out:fade|local={{ duration: motion.duration }}>
            {#if (coverOn || parent) && !pageFull}
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
              <div class="cover" transition:fade|global={motion}>
                <Settings bind:section={settingsSection} on:setupRemote={openRemoteSetup} onSetup={openSetup} onReport={reportProblem} />
              </div>
            {:else if coverOn && cover === "page" && coverPage}
              <div class="cover" class:under-dock={!pageFull} style:--dock="{dockHeight}px" transition:fade|global={motion}>
                {#key coverPage.id}
                  <PageView
                    tab={coverPage}
                    {immersive}
                    devices={phonesWork}
                    on:navigate={(e) => coverPage && navigatePage(coverPage.id, e.detail.url)}
                    on:error={(e) => toast(e.detail.text, true)}
                    on:openTab={(e) => coverPage && navigatePage(coverPage.id, e.detail.url)}
                    on:recover={() => coverPage && recoverPage(coverPage.id)}
                    on:resume={() => coverPage && patchPage(coverPage.id, { crashed: undefined })}
                  />
                {/key}
              </div>
            {:else if coverOn && cover === "file" && coverDoc}
              <div class="cover" style:--dock="{dockHeight}px" transition:fade|global={motion}>
                <FileReader doc={coverDoc} {folder} full on:close={clearCover} />
              </div>
            {:else if coverOn && cover === "sessions"}
              <div class="cover" transition:fade|global={motion}>
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
            {:else if hasSession}
            <div class="workarea">
              <div class="scroll" style:--dock="{dockHeight}px" bind:this={scrollEl} use:follow.attach>
                <Thread
                  {events}
                  liveText={live}
                  {liveReasoning}
                {liveTodo}
                  {liveArtifacts}
                  plan={meta?.plan ?? null}
                  draft={meta?.status === "draft" ? { title: meta.title, brief: meta.brief ?? "" } : null}
                  draftStarting={sending}
                  on:startDraft={startDraft}
                  on:removeDraft={() => shown && void remove(shown)}
                  {liveTools}
                  {liveHelpers}
                  {approval}
                  question={openQuestion}
                  {proposal}
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
                  on:proposal={(e) => void answerProposal(e.detail.session)}
                  on:edited={(e) => {
                    comp.input = e.detail.text;
                    comp = comp;
                    omnibar?.focus();
                    toast("Loaded into composer. Edit and send.");
                  }}
                  on:forked={() => fork()}
                  on:retry={retryLast}
                  on:compact={compact}
                  on:reply={(e) => void sendReply(e.detail.text)}
                on:rewind={(e) => void rewindTo(e.detail.nth, e.detail.text)}
                on:pickModel={pickAndRetry}
                  on:openSession={(e) => openSession(e.detail.id)}
                />
              </div>
            </div>
            {/if}
          </section>
          {/if}
          {#if readerShown && fileDoc}
            <FileReader doc={fileDoc} {folder} width={readerWidth} on:close={closeFile} on:resize={resizeFile} />
          {/if}
          <SessionInfo
            session={shown ?? ""}
            project={sessionProject}
            projectTitle={projectTitle}
            {folder}
            {branch}
            remoteHost={remoteOn ? ($remoteInfo?.host ?? "") : ""}
            contextUsed={context.used}
            contextLimit={context.limit}
            {compacting}
            {streaming}
            {threads}
            running={working}
            {approvals}
            questions={asks}
              width={infoWidth}
            open={infoShown}
            on:resize={resizeInfo}
            tokensIn={meta?.tokens_in ?? 0}
            tokensOut={meta?.tokens_out ?? 0}
            costUsd={meta?.cost_usd ?? 0}
            provider={(comp.model || meta?.model || "").split("/")[0]}
            on:openSession={(e) => openSession(e.detail.id)}
            on:compact={compact}
            on:moved={(e) => relocated(e.detail.meta)}
          />
        </div>
      {/if}

      {#if (!coverOn || coverComposer) && !pageFull}
      <div
        class="composer"
        class:docked={hasSession || coverOn}
        style:--shift="{shift}px"
        bind:clientHeight={dockHeight}
      >
        {#if tab.kind === "session" && shown && hasSession}
          <SessionPins
            session={shown}
            project={sessionProject}
            planDoc={meta?.plan?.doc ?? null}
            current={coverComposer ? coverTarget : ""}
            on:open={(e) => openPin(e.detail)}
          />
        {/if}
        <Omnibar
          bind:this={omnibar}
          bind:input={comp.input}
          bind:model={comp.model}
          bind:effort={comp.effort}
          bind:permission
          {policy}
          bind:mode={comp.mode}
          bind:attachments={comp.attachments}
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
          hero={!hasSession && !coverOn}
          hint={proposal ? "Say what to change, or accept the plan above" : ""}
          {project}
          queue={heldRows}
          queuePaused={queueIsPaused}
          on:send={() => void send()}
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
          on:usage={() => openSettings("providers")}
          on:folder={(e) => patchTab(tab.id, { cwd: e.detail.path })}
          on:project={(e) => loadProject(e.detail.path ?? tab.cwd ?? folder)}
        />
      </div>
      {/if}
    {/if}
    </main>
  </div>

  <ImageViewer />

  {#if setupOpen}
    <Onboarding startStep={setupStep} server={setupServer} on:close={() => (setupOpen = false)} />
  {/if}

  <div class="toasts" class:chrome={toastsUp} aria-live="polite">
    {#each toastsUp ? $toasts.slice(-1) : $toasts as t (t.id)}
      {#if t.session}
        <button
          class="toast go"
          transition:fade={{ duration: DUR }}
          on:click={() => {
            dismissToast(t.id);
            if (immersive) void setImmersive(false);
            openSession(t.session ?? "");
          }}>{t.text}</button
        >
      {:else}
        <div class="toast" class:err={t.err} class:acts={!!t.action} transition:fade={{ duration: DUR }}>
          <span class="toast-text">{t.text}</span>
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
  /* The card look is one set of values for all of
     Parzi (theme.css, --surface and --blur); on Home it
     eases as it changes. */
  .on-home :global(:is(.info .card, .reader .card, .ob .box)) {
    transition:
      background var(--dur) ease,
      border-color var(--dur) ease,
      backdrop-filter var(--dur) ease;
  }
  /* Onto or off the wallpaper: the change eases over about the picture's
     fade, so glass turns solid and outlines come in rather than snap. */
  .art-shift :global(:is(.info .card, .reader .card, .ob .box, .tabs .tab, .panel-btn, .info .card .btn)) {
    transition:
      background 480ms var(--ease-even),
      border-color 480ms var(--ease-even),
      color 480ms var(--ease-even),
      backdrop-filter 480ms var(--ease-even),
      -webkit-backdrop-filter 480ms var(--ease-even),
      transform 160ms ease;
  }
  /* Light theme, picture filling the window: Home is a dark scene (the
     theme marks data-scene). Over the picture the theme's colours swap,
     its text colour the ground and its ground the text, so the title bar
     and the line above the composer read as on a dark theme. Cards and the
     composer keep the light theme's own colours: light frosted glass with
     dark text, like light widgets on a dark wallpaper. The open tab and
     toggles that are on are the same light glass. */
  :global(html[data-scene="dark"]) .over-art {
    --bg: var(--scene-bg);
    --text: var(--scene-text);
    --accent: var(--scene-accent);
  }
  :global(html[data-scene="dark"]) .over-art :global(:is(.info .card, .reader .card, .ob .box, .tabs .tab.active, .tabs .tab:hover, .panel-btn.active, .panel-btn:hover)) {
    --bg: var(--scene-text);
    --text: var(--scene-bg);
    --accent: var(--theme-accent);
    color-scheme: light;
  }
  /* The colours made from the pair, made again wherever it changes. */
  :global(html[data-scene="dark"]) .over-art,
  :global(html[data-scene="dark"]) .over-art :global(:is(.info .card, .reader .card, .ob .box, .tabs .tab.active, .tabs .tab:hover, .panel-btn.active, .panel-btn:hover)) {
    --panel: color-mix(in srgb, var(--text) 6%, var(--bg));
    --muted: color-mix(in srgb, var(--text) 78%, var(--bg));
    --line: color-mix(in srgb, var(--text) 12%, var(--bg));
    --faint: color-mix(in srgb, var(--muted) 62%, var(--bg));
    --hover: color-mix(in srgb, var(--text) 6%, transparent);
    --fill: color-mix(in srgb, var(--text) 8%, transparent);
    --selected: color-mix(in srgb, var(--text) 10%, transparent);
    --fill-hover: color-mix(in srgb, var(--text) 14%, transparent);
    --surface: color-mix(in srgb, var(--panel) 52%, transparent);
    --surface-raised: color-mix(in srgb, var(--panel) 72%, transparent);
    --edge: color-mix(in srgb, var(--text) 9%, transparent);
    --link: color-mix(in srgb, var(--accent) 70%, var(--text));
    --ok-text: color-mix(in srgb, var(--ok) 40%, var(--text));
    --warn-text: color-mix(in srgb, var(--warn) 35%, var(--text));
    --bad-text: color-mix(in srgb, var(--bad) 62%, var(--text));
    --accent-hover: color-mix(in srgb, var(--accent) 86%, var(--text));
    color: var(--text);
  }
  :global(html[data-scene="dark"]) .over-art {
    color-scheme: dark;
  }
  /* Light glass holds more of its own colour than dark glass: at half
     strength it turns grey over a dimmed picture. */
  :global(html[data-scene="dark"]) .over-art :global(:is(.info .card, .reader .card, .ob .box, .tabs .tab.active, .tabs .tab:hover, .panel-btn.active, .panel-btn:hover)) {
    --surface: color-mix(in srgb, var(--panel) 80%, transparent);
  }
  /* Light theme, text straight on the picture: the full text colour instead
     of grey, with a hairline edge in the page colour, not a glow. */
  :global(html[data-tone="light"]) .over-art :global(.topbar),
  :global(html[data-tone="light"]) .over-art :global(.ob .where) {
    --muted: var(--text);
  }
  :global(html[data-tone="light"]) .over-art :global(.tabs .tab),
  :global(html[data-tone="light"]) .over-art :global(.ob .where) {
    text-shadow: 0 0 1px color-mix(in srgb, var(--bg) 70%, transparent);
  }
  :global(html[data-tone="light"]) .over-art :global(.topbar svg) {
    filter: drop-shadow(0 0 1px color-mix(in srgb, var(--bg) 70%, transparent));
  }
  /* Tabs over the picture: the open tab and the side-column toggle when
     on are the cards' glass; under the pointer, a lighter frost. */
  .over-art :global(.tabs .tab:is(.active, :hover)),
  .over-art :global(.panel-btn:is(.active, :hover)) {
    background: var(--surface);
    border-color: transparent;
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
  }
  .over-art :global(.tabs .tab:hover:not(.active)),
  .over-art :global(.panel-btn:hover:not(.active)) {
    background: var(--fill-hover);
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
    background: var(--surface);
    border: 1px solid var(--edge);
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
  /* The stage (a session, a page, an artifact) is a card inset from the
     window, its rounded top corners and sides drawn by one faint edge: with
     no edge, the shaded top hid half its outline and the corner read as a
     stray strip down the left. */
  .session {
    position: relative;
    min-width: 0;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap);
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--edge);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    animation: enter 600ms var(--ease-out);
  }
  .page-card {
    min-width: 0;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap);
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--edge);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    animation: enter 600ms var(--ease-out);
  }
  .page-card :global(.page) {
    flex: 1;
    min-height: 0;
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
  .cover :global(nav),
  .cover :global(header),
  .cover :global(.bar),
  .cover :global(.toolbar),
  .cover :global(.page-bar) {
    anchor-name: --cover-bar;
  }
  /* Room for the back arrow. A bar on the page's tracks already stays clear of it. */
  .cover :global(nav:not(.tracks)),
  .cover :global(header),
  .cover :global(.bar),
  .cover :global(.toolbar) {
    padding-right: calc(var(--space) * 4 + var(--space) * 7 + var(--space) * 2);
  }
  .session:has(.cover) .back {
    position-anchor: --cover-bar;
    top: anchor(center, calc(var(--space) * 2 + 14px));
    translate: 0 -50%;
  }
  /* Out of flow: while one cover fades into another, neither takes space from the other. */
  .cover {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  /* A page's native view is drawn above the window's HTML, so it stops
     short of the composer floating at the foot. */
  .cover.under-dock {
    padding-bottom: calc(var(--dock) + var(--space) * 6);
  }
  .cover :global(.page) {
    flex: 1;
    min-height: 0;
  }
  .scroll {
    flex: 1;
    overflow-x: hidden;
    overflow-y: auto;
    padding-bottom: calc(var(--dock) + calc(var(--space) * 8));
    user-select: text;
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
    background: var(--surface-raised);
    border: 1px solid var(--edge);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
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
    color: var(--bad-text);
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
  .toast-act {
    flex: none;
    min-height: var(--h-sm);
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
    min-height: var(--h);
    padding: var(--space) calc(var(--space) * 2) var(--space) calc(var(--space) * 3);
    white-space: nowrap;
  }
  .toasts.chrome .toast:not(.acts) {
    padding-right: calc(var(--space) * 3);
  }
</style>
