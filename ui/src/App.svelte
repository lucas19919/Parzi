<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api, brain, deskSync, onBrowser, onBrowserKey, onBrowserOpen, onDesk, onRunEvent,
    MODE_META, type ChatEvent, type ComposerMode, type PageEvent, type SessionMeta, type UiEvent,
  } from "./lib/api";
  import TopBar from "./lib/TopBar.svelte";
  import Switcher from "./lib/Switcher.svelte";
  import DefaultArt from "./lib/DefaultArt.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import Thread from "./lib/Thread.svelte";
  import SessionHeader from "./lib/SessionHeader.svelte";
  import PageView from "./lib/PageView.svelte";
  import HistoryView from "./lib/HistoryView.svelte";
  import HomeView from "./lib/HomeView.svelte";
  import BrainView from "./lib/BrainView.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import { loadTabs, onboarded, recordVisit, saveTabs, titleVisit } from "./lib/browserData";
  import Settings from "./lib/Settings.svelte";
  import Icon from "./lib/Icon.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce } from "./lib/threadList";
  import { checkForUpdatesSoon } from "./lib/updateStore";
  import { brainTab, historyTab, hostOf, isExplicitUrl, pageTab, sessionTab, toAddress, type Tab } from "./lib/tabs";
  import { toast, toastError, toasts, notify } from "./lib/toast";
  import { covered } from "./lib/overlay";
  import type { Approval, LiveTool } from "./lib/live";

  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motion = reduced ? { duration: 0 } : { duration: 200, easing: cubicOut };

  const restored = loadTabs();
  let tabs: Tab[] = restored?.tabs ?? [sessionTab()];
  let activeId = restored?.active ?? tabs[0].id;
  let threads: SessionMeta[] = [];
  let bg = "";

  let shown: string | null = null;
  let meta: SessionMeta | null = null;
  let events: ChatEvent[] = [];
  let running = new Set<string>();
  let approvals: Approval[] = [];
  let context = { used: 0, limit: 0 };
  let compacting = false;
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTools: LiveTool[] = [];
  let pending = "";
  let flushTimer = 0;

  let input = "";
  let model = "auto";
  let warmed = "";
  let effort = "medium";
  let permission = "full";
  let mode: ComposerMode = "code";
  let attachments: string[] = [];
  let sending = false;

  const modeDefaults = { search: { model: "auto", effort: "medium" }, code: { model: "auto", effort: "medium" }, research: { model: "auto", effort: "low" } };
  let modeKept: Record<ComposerMode, { model: string; effort: string }> = {
    search: { ...modeDefaults.search },
    code: { ...modeDefaults.code },
    research: { ...modeDefaults.research },
  };
  let prevMode: ComposerMode = mode;

  function laneOf(sessionId: string | null | undefined): string {
    if (!sessionId) return "";
    return threads.find((t) => t.id === sessionId)?.lane ?? "";
  }

  $: if (mode !== prevMode) {
    modeKept[prevMode] = { model, effort };
    const kept = modeKept[mode] ?? modeDefaults[mode];
    model = kept.model;
    effort = kept.effort;
    prevMode = mode;
  }

  let switcherOpen = false;
  let settingsOpen = false;
  let settingsSection = "general";
  let scrollEl: HTMLElement | null = null;
  let farFromBottom = false;
  let dockHeight = 90;
  let omnibar: Omnibar;
  let page: PageView;
  let immersive = false;
  let setupOpen = false;
  let project: { slug: string; title: string; tokens: number } | null = null;
  let navSeq = 0;
  let deskRev = 0;

  $: tab = tabs.find((t) => t.id === activeId) ?? tabs[0];
  $: streaming = !!shown && running.has(shown);
  $: hasSession = tab.kind === "session" && (!!shown || sending || events.length > 0);
  $: folder = tab.kind === "session" && tab.sessionId ? (meta?.cwd ?? "") : (tab.cwd ?? "");
  $: approval = approvals.find((a) => a.session === shown) ?? null;
  $: void loadBranch(folder);
  $: void loadProject(folder);
  $: syncDesk(tabs, activeId);
  $: saveTabs(tabs, activeId);
  $: pageVisible = tab.kind === "page" && !settingsOpen;
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

  let historyView: "sessions" | "history" | "bookmarks" | "agents" = "sessions";

  function openHistory(view = historyView) {
    historyView = view;
    settingsOpen = false;
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

  function openBrain() {
    settingsOpen = false;
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
      if (m.model.includes("/")) model = m.model;
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
    activeId = id;
    settingsOpen = false;
    if (next.kind === "session") {
      if (next.sessionId) void showSession(next.sessionId);
      else showDraft();
    }
  }

  function addTab(t: Tab) {
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
    if (tabs.length === 1) {
      if (tabs[0].kind === "page") api.browserClose(id).catch(() => {});
      tabs = [sessionTab()];
      selectTab(tabs[0].id);
      return;
    }
    if (tabs[idx].kind === "page") {
      api.browserClose(id).catch(() => {});
      const t = tabs[idx];
      if (t.url) closed = [...closed, { url: t.url, title: t.title }].slice(-20);
    }
    tabs = tabs.filter((t) => t.id !== id);
    if (activeId === id) selectTab(tabs[Math.max(0, idx - 1)].id);
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

  function openPage(url: string, id?: string) {
    const existing = tabs.find((t) => (id ? t.id === id : t.kind === "page" && t.url === url));
    if (existing) {
      if (url && existing.url !== url) patchTab(existing.id, { url, title: hostOf(url) });
      selectTab(existing.id);
    } else {
      addTab(pageTab(url, id));
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

  function openSettings(section = "general") {
    settingsSection = section;
    switcherOpen = false;
    settingsOpen = true;
  }

  async function send() {
    const prompt = input.trim();
    if (!prompt || sending || streaming) return;
    if (isExplicitUrl(prompt)) {
      input = "";
      browse(toAddress(prompt));
      return;
    }
    const target = tab;
    const files = attachments;
    const sessionLane = laneOf(target.sessionId);
    const fresh = !target.sessionId || (sessionLane !== "" && sessionLane !== mode);
    const optimistic: ChatEvent = { kind: "user", text: prompt };
    sending = true;
    input = "";
    attachments = [];
    events = [...events, optimistic];
    try {
      const sid = await api.sendMessage({
        sessionId: fresh ? null : target.sessionId,
        model,
        prompt,
        cwd: folder,
        effort,
        attachments: files,
        mode: permission,
        lane: mode,
      });
      running = new Set(running).add(sid);
      if (fresh) patchTab(target.id, { sessionId: sid, title: prompt.slice(0, 40) });
      if (activeId === target.id) {
        shown = sid;
        await reload();
        await scrollToBottom();
      }
      void refreshThreads();
    } catch (e) {
      input = prompt;
      attachments = files;
      events = events.filter((ev) => ev !== optimistic);
      toastError(e);
    } finally {
      sending = false;
    }
  }

  async function stop() {
    if (!shown || !running.has(shown)) return;
    await stopSession(shown);
    clearLive();
    await reload();
    void refreshThreads();
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
    try {
      await api.compactThread(shown);
      await reload();
    } catch (e) {
      toastError(e);
    } finally {
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
    if (e.kind === "done" || e.kind === "error") {
      running.delete(e.session);
      running = running;
      approvals = approvals.filter((a) => a.session !== e.session);
      void refreshThreads();
      if (e.session === shown) {
        flushLive();
        clearLive();
        void reload();
      } else {
        const title = threads.find((t) => t.id === e.session)?.title || "Agent";
        notify(e.kind === "done" ? `${title} finished` : `${title} stopped`, e.session);
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
    else if (name === "ctrl+comma") settingsOpen ? (settingsOpen = false) : openSettings();
    else if (name === "ctrl+b") openBrain();
    else if (name === "ctrl+h") openHistory();
    else if (name === "ctrl+shift+a") openHistory("agents");
    else if (name === "ctrl+shift+t") reopenClosed();
    else if (name === "f11") void setImmersive(!immersive);
    else if (name === "ctrl+l") {
      if (tab.kind === "page" && !settingsOpen) {
        void setImmersive(false);
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
      else if (settingsOpen) settingsOpen = false;
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
    const unRun = onRunEvent(onEvent);
    const unDesk = onDesk((cmd) => {
      if (typeof cmd.rev === "number") deskRev = Math.max(deskRev, cmd.rev);
      if (cmd.op === "open" && cmd.url) openPage(cmd.url, cmd.id);
      else if (cmd.op === "focus" && cmd.id) selectTab(cmd.id);
      else if (cmd.op === "close" && cmd.id) closeTab(cmd.id);
    });
    const unPage = onBrowser(onPage);
    const unOpen = onBrowserOpen((e) => openPageNext(e.url));
    const unKey = onBrowserKey((name) => void shortcut(name));
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
      unRun.then((f) => f()).catch(() => {});
      unDesk.then((f) => f()).catch(() => {});
      for (const un of [unPage, unOpen, unKey]) un.then((f) => f()).catch(() => {});
      document.removeEventListener("parzi:bg", onBg);
    };
  });
</script>

<svelte:window on:keydown={onKey} />

<div class="shell">
  <DefaultArt {bg} blurred={hasSession || tab.kind === "page" || settingsOpen} />

  {#if !hasSession && !settingsOpen && tab.kind !== "page" && tab.kind !== "history" && tab.kind !== "brain"}
    <div class="hero-reef" aria-hidden="true" />
  {/if}

  {#if !immersive}
  <TopBar
    {tabs}
    activeTabId={activeId}
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

  <main>
    {#if settingsOpen}
      <div class="fill" in:fly={{ y: 8, ...motion }}>
        <Settings bind:section={settingsSection} on:close={() => (settingsOpen = false)} />
      </div>
    {:else if tab.kind === "brain"}
      <div class="fill" in:fade={{ duration: 150 }}>
        <BrainView />
      </div>
    {:else if tab.kind === "history"}
      <div class="fill" in:fade={{ duration: 150 }}>
        <HistoryView
          {threads}
          {running}
          bind:view={historyView}
          on:open={(e) => openPageNext(e.detail.url)}
          on:openSession={(e) => openSession(e.detail.id)}
          on:deleteSession={(e) => remove(e.detail.id)}
          on:stopSession={(e) => stopSession(e.detail.id)}
          on:refresh={() => void refreshThreads()}
        />
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
            canAct={!!shown}
            on:rename={(e) => rename(e.detail.title)}
            on:fork={fork}
            on:copyId={copyId}
            on:delete={() => shown && remove(shown)}
          />
          <div class="scroll" bind:this={scrollEl} on:scroll={() => (farFromBottom = !nearBottom())}>
            <Thread
              {events}
              liveText={live}
              {liveReasoning}
              {liveTools}
              {approval}
              {streaming}
              {folder}
              on:voted={(e) => (approvals = approvals.filter((a) => a.key !== e.detail.key))}
            />
          </div>
          {#if farFromBottom}
            <button class="to-bottom" title="Scroll to bottom" transition:fade={{ duration: 120 }} on:click={() => scrollToBottom(true)}>
              <Icon name="arrowDown" size={14} stroke={2.2} />
            </button>
          {/if}
        </section>
      {/if}

      {#if !hasSession}
        <div class="home" in:fade={{ duration: 200 }}>
          <HomeView
            {threads}
            on:open={(e) => browse(e.detail.url)}
            on:openSession={(e) => openSession(e.detail.id)}
            on:allSessions={() => openHistory("sessions")}
          />
        </div>
      {/if}

      <div class="composer" class:docked={hasSession} bind:clientHeight={dockHeight}>
        {#if hasSession && meta?.lane && meta.lane !== mode && mode !== "search"}
          <div class="lane-hint">↵ starts a new {MODE_META[mode].label} session</div>
        {/if}
        <Omnibar
          bind:this={omnibar}
          bind:input
          bind:model
          bind:effort
          bind:permission
          bind:mode
          bind:attachments
          {folder}
          folderLocked={!!tab.sessionId}
          {branch}
          {streaming}
          board={$board}
          contextUsed={context.used}
          contextLimit={context.limit}
          {compacting}
          hero={!hasSession}
          {project}
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
  }
  :global(html.parzi-maximized) .shell {
    border-radius: 0;
  }
  main {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .fill {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .session {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 8px 8px 0;
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line);
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
  .to-bottom {
    position: absolute;
    bottom: calc(var(--dock) + 26px);
    left: 50%;
    z-index: 15;
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transform: translateX(-50%);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 50%;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    color: var(--muted);
    cursor: pointer;
  }
  .to-bottom:hover {
    color: var(--text);
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
    bottom: calc(50% + 20px);
    z-index: 20;
    width: min(720px, 90%);
    transform: translate(-50%, 0);
    transition:
      bottom 1050ms cubic-bezier(0.22, 1, 0.36, 1),
      transform 1050ms cubic-bezier(0.22, 1, 0.36, 1),
      width 700ms ease;
  }
  .composer.docked {
    bottom: 14px;
    width: min(740px, calc(100% - 40px));
    transform: translate(-50%, 0);
  }
  .lane-hint {
    margin: 0 0 6px;
    text-align: center;
    font-size: 11.5px;
    color: var(--faint);
  }
  .hero-reef {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 0;
    height: 48%;
    pointer-events: none;
    background: url("/hero-harbor.png") center 32% / cover no-repeat;
    -webkit-mask-image: linear-gradient(to bottom, #000 58%, transparent 99%);
    mask-image: linear-gradient(to bottom, #000 58%, transparent 99%);
    opacity: 0.9;
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
