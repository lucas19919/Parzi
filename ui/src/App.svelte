<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    api, hub, onRunEvent,
    type SessionMeta, type ChatEvent, type UiEvent,
    type ProjectView, type ProjectRoster,
  } from "./lib/api";

  import TopBar from "./lib/TopBar.svelte";
  import type { Tab } from "./lib/tabTypes";
  import SessionPopup from "./lib/SessionPopup.svelte";
  import BrowserView from "./lib/BrowserView.svelte";
  import DefaultArt from "./lib/DefaultArt.svelte";
  import Omnibar from "./lib/Omnibar.svelte";
  import Thread from "./lib/Thread.svelte";
  import Settings from "./lib/Settings.svelte";
  import { board, ensureBoard } from "./lib/providerStore";
  import { effortLabel, effortsFor } from "./lib/providerRows";
  import { applyThemeCss } from "./lib/theme";
  import { coalesce, changesThreadList } from "./lib/threadList";
  import { checkForUpdates, checkForUpdatesSoon } from "./lib/updateStore";

  const RM = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const smooth = RM ? { duration: 0 } : { duration: 200, easing: cubicOut };
  const smoothFast = RM ? { duration: 0 } : { duration: 140, easing: cubicOut };

  let bg = "";
  let threads: SessionMeta[] = [];
  let projects: ProjectView[] = [];

  let curProject = "default";
  let projectRoster: ProjectRoster | null = null;
  let activeThreadId: string | null = null;
  let activeMeta: SessionMeta | null = null;
  let events: ChatEvent[] = [];
  let branch = "";

  let live = "";
  let liveReasoning = "";
  let liveTokens = 0;
  let liveCost = 0;
  let compacting = false;

  $: contextUsed = activeMeta?.context_tokens ?? 0;
  $: contextLimit = activeMeta?.context_limit ?? 0;

  let liveTools: { id: string; name: string; label: string; running: boolean; ok: boolean; ms: number }[] = [];
  let input = "";
  let model = "auto";
  let effort = "medium";
  let permission = "full";

  function pillMode(): string {
    if (permission === "supervised" || permission === "edits") return permission;
    return "auto";
  }
  let curLane = "";
  let attachments: string[] = [];

  // Settings
  let showSettings = false;
  let settingsSection = "general";

  function openSettings(section = "general") {
    settingsSection = section;
    showSessionPopup = false;
    showSettings = true;
  }

  // Workspaces
  let hubTick = 0;
  let wsNames: string[] = [];
  let wsRoots: Record<string, string> = {};

  async function loadWorkspaces() {
    const my = ++dataSeq;
    try {
      wsNames = await hub.workspaces();
    } catch {
      wsNames = [];
    }
    if (my !== dataSeq) return;
    const pairs = await Promise.all(
      wsNames.map(async (n) => {
        const w = await hub.workspace(n).catch(() => null);
        return [n, w?.repos.find((r) => r.local_path)?.local_path ?? ""] as const;
      }),
    );
    if (my !== dataSeq) return;
    wsRoots = Object.fromEntries(pairs);
  }
  $: if (hubTick >= 0) void loadWorkspaces();
  $: curWorkspace = wsNames.includes(curProject) ? curProject : "";

  // Dual-mode Tabs system
  let tabs: Tab[] = [
    {
      id: "tab-1",
      kind: "harness",
      title: "New session",
      badge: "W",
      sessionId: null,
    },
  ];
  let activeTabId: string = "tab-1";
  $: currentTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];

  // Session Popup Handler (Spotlight)
  let showSessionPopup = false;
  let sessionMenuOpen = false;

  function toggleSessionMenu() {
    sessionMenuOpen = !sessionMenuOpen;
  }

  function handleSelectTab(id: string) {
    activeTabId = id;
    const tab = tabs.find((t) => t.id === id);
    if (!tab) return;
    if (tab.kind === "harness") {
      if (tab.sessionId) {
        void openThread(tab.sessionId, false);
      } else {
        activeThreadId = null;
        activeMeta = null;
        events = [];
        clearLive();
      }
    }
  }

  function handleCloseTab(id: string) {
    if (tabs.length <= 1) {
      tabs = [
        {
          id: "tab-" + Date.now(),
          kind: "harness",
          title: "New session",
          badge: "W",
          sessionId: null,
        },
      ];
      activeTabId = tabs[0].id;
      activeThreadId = null;
      activeMeta = null;
      events = [];
      clearLive();
      return;
    }
    const idx = tabs.findIndex((t) => t.id === id);
    const wasActive = activeTabId === id;
    tabs = tabs.filter((t) => t.id !== id);
    if (wasActive) {
      const nextIdx = Math.max(0, idx - 1);
      const nextTab = tabs[nextIdx];
      activeTabId = nextTab.id;
      if (nextTab.kind === "harness") {
        if (nextTab.sessionId) {
          void openThread(nextTab.sessionId, false);
        } else {
          activeThreadId = null;
          activeMeta = null;
          events = [];
          clearLive();
        }
      }
    }
  }

  function handleNewTab() {
    const newId = "tab-" + Date.now();
    const newTabItem: Tab = {
      id: newId,
      kind: "harness",
      title: "New session",
      badge: curWorkspace ? "W" : "I",
      sessionId: null,
    };
    tabs = [...tabs, newTabItem];
    activeTabId = newId;
    activeThreadId = null;
    activeMeta = null;
    events = [];
    clearLive();
    showSettings = false;
  }

  function handleNewBrowserTab(initialUrl = "https://duckduckgo.com") {
    const newId = "tab-" + Date.now();
    const newTabItem: Tab = {
      id: newId,
      kind: "browser",
      title: "Web Browser",
      badge: "B",
      url: initialUrl,
    };
    tabs = [...tabs, newTabItem];
    activeTabId = newId;
    showSettings = false;
  }

  let omniMode: "agent" | "web" = "agent";

  function handleOmniBrowse(e: CustomEvent<{ url: string }>) {
    const targetUrl = e.detail.url;
    const cur = tabs.find((t) => t.id === activeTabId);
    const domain = targetUrl.replace(/^https?:\/\/(www\.)?/, "").replace(/\/$/, "").slice(0, 24);
    if (cur && cur.kind === "harness" && !cur.sessionId && events.length === 0) {
      cur.kind = "browser";
      cur.badge = "B";
      cur.url = targetUrl;
      cur.title = domain || "Web Browser";
      tabs = [...tabs];
    } else {
      handleNewBrowserTab(targetUrl);
    }
  }

  function handleOpenSession(id: string) {
    const existing = tabs.find((t) => t.sessionId === id);
    if (existing) {
      activeTabId = existing.id;
      void openThread(id, false);
    } else {
      const cur = tabs.find((t) => t.id === activeTabId);
      if (cur && !cur.sessionId && cur.kind === "harness") {
        cur.sessionId = id;
        cur.title = threads.find((t) => t.id === id)?.title || "Session";
        tabs = [...tabs];
        void openThread(id, false);
      } else {
        const meta = threads.find((t) => t.id === id);
        const newId = "tab-" + Date.now();
        const newTabItem: Tab = {
          id: newId,
          kind: "harness",
          title: meta?.title || "Session",
          badge: meta?.lane ? meta.lane.slice(0, 1).toUpperCase() : "W",
          sessionId: id,
        };
        tabs = [...tabs, newTabItem];
        activeTabId = newId;
        void openThread(id, false);
      }
    }
    showSettings = false;
  }

  function handleSelectWorkspace(name: string) {
    void selectWorkspace(name);
  }

  async function selectWorkspace(name: string) {
    if (name === curProject) return;
    curProject = name;
    activeThreadId = null;
    activeMeta = null;
    events = [];
    clearLive();
    toast(`Workspace: ${name}`);
    await loadThreads();
  }

  // Toasts
  interface Toast {
    id: number;
    text: string;
    err: boolean;
  }
  let toasts: Toast[] = [];
  let toastSeq = 0;
  function toast(text: string, err = false) {
    const id = ++toastSeq;
    toasts = [...toasts, { id, text, err }];
    setTimeout(() => {
      toasts = toasts.filter((t) => t.id !== id);
    }, 3500);
  }

  // Approval card
  interface ApprovalCard {
    key: string;
    session: string;
    call: { id: string; name: string; args: unknown; lane: string; session: string };
  }
  let approvals: ApprovalCard[] = [];
  $: activeApproval = approvals.find((a) => a.session === activeThreadId) ?? null;

  let liveRun: string | null = null;
  let sending = false;
  let scrollEl: HTMLElement | null = null;
  let showScrollBottom = false;

  function onStageScroll() {
    if (!scrollEl) return;
    const dist = scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight;
    showScrollBottom = dist > 140;
  }

  function scrollToBottom() {
    if (scrollEl) {
      scrollEl.scrollTo({ top: scrollEl.scrollHeight, behavior: RM ? "auto" : "smooth" });
    }
  }

  $: currentRoot = (wsRoots[curWorkspace] || wsRoots[curProject] || "").trim();

  // Stage blur & background fade
  let isBlurredStage = false;
  $: isBlurredStage = !!activeThreadId || sending || events.length > 0 || (currentTab?.kind === "browser" && !!currentTab?.url) || showSettings;

  let navSeq = 0;
  let dataSeq = 0;

  async function openThread(id: string, keepLive = false) {
    const my = ++navSeq;
    activeThreadId = id;
    if (!keepLive) clearLive();
    liveTokens = 0;
    liveCost = 0;
    try {
      const [meta, ev] = await api.getThread(id);
      if (my !== navSeq || activeThreadId !== id) return;
      activeMeta = meta;
      events = ev;
      curProject = meta.project || "default";
      curLane = meta.lane || "";
      if (meta.model && meta.model.includes("/")) {
        model = meta.model;
      }
      // Update tab title
      const cur = tabs.find((t) => t.id === activeTabId);
      if (cur) {
        cur.title = meta.title || "Session";
        cur.sessionId = id;
        tabs = [...tabs];
      }
    } catch (e) {
      if (my !== navSeq || activeThreadId !== id) return;
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
    handleNewTab();
  }

  async function send() {
    const rawPrompt = input.trim();
    if (!rawPrompt || sending || liveRun) return;

    // Smart URL Detection: If user entered a URL, route to web browser tab!
    const isUrl = /^https?:\/\//i.test(rawPrompt) ||
                  /^(localhost|\w+\.\w+)/i.test(rawPrompt) ||
                  /^\/(browse|web)\s+/i.test(rawPrompt);

    if (isUrl) {
      let targetUrl = rawPrompt.replace(/^\/(browse|web)\s+/i, "").trim();
      if (!/^https?:\/\//i.test(targetUrl)) {
        targetUrl = "https://" + targetUrl;
      }
      input = "";
      const cur = tabs.find((t) => t.id === activeTabId);
      if (cur) {
        cur.kind = "browser";
        cur.url = targetUrl;
        cur.title = targetUrl.replace(/^https?:\/\/(www\.)?/, "").slice(0, 24);
        tabs = [...tabs];
      } else {
        handleNewBrowserTab(targetUrl);
      }
      return;
    }

    const my = navSeq;
    sending = true;
    input = "";
    const files = [...attachments];
    attachments = [];
    const cwd = currentRoot;
    const prompt = rawPrompt;

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
      if (my !== navSeq) {
        await loadThreads();
        return;
      }
      activeThreadId = sid;
      liveRun = sid;
      live = "";
      liveReasoning = "";
      liveTools = [];

      // Update current tab
      const cur = tabs.find((t) => t.id === activeTabId);
      if (cur) {
        cur.sessionId = sid;
        cur.title = prompt.slice(0, 24);
        tabs = [...tabs];
      }

      await loadThreads();
      try {
        const [meta, ev] = await api.getThread(sid);
        if (my !== navSeq) return;
        activeMeta = meta;
        events = ev;
        curProject = meta.project || curProject;
        curLane = meta.lane || curLane;
        if (cur && meta.title) {
          cur.title = meta.title;
          tabs = [...tabs];
        }
      } catch {}
      await tick();
      if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
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
      toast("Forked session");
    } catch (e) {
      toast(String(e), true);
    }
  }

  async function stopRun() {
    if (liveRun) {
      const id = liveRun;
      try {
        await api.killRun(id);
        if (liveRun === id) {
          liveRun = null;
          clearLive();
          if (activeThreadId) await refreshEvents();
          await loadThreads();
        }
        toast("Agent stopped");
      } catch (e) {
        toast(String(e), true);
      }
    }
  }

  async function handleDeleteThread(id: string) {
    try {
      await api.deleteThread(id);
      if (liveRun === id) {
        liveRun = null;
        clearLive();
      }
      // Remove from tabs if present
      tabs = tabs.filter((t) => t.sessionId !== id);
      if (tabs.length === 0) {
        handleNewTab();
      } else {
        handleSelectTab(tabs[0].id);
      }
      await loadThreads();
      toast("Session deleted");
    } catch (e) {
      toast(String(e), true);
    }
  }

  function handleRenameSession() {
    sessionMenuOpen = false;
    if (!activeThreadId) return;
    const current = activeMeta?.title || "Session";
    const next = window.prompt("Rename session:", current);
    if (next && next.trim() && next !== current) {
      const title = next.trim();
      api.renameThread(activeThreadId, title)
        .then(() => {
          if (activeMeta) activeMeta.title = title;
          const cur = tabs.find((t) => t.id === activeTabId);
          if (cur) {
            cur.title = title;
            tabs = [...tabs];
          }
          toast("Renamed");
          return loadThreads();
        })
        .catch((e) => toast(String(e), true));
    }
  }

  function handleCopyId() {
    sessionMenuOpen = false;
    if (activeThreadId) {
      navigator.clipboard.writeText(activeThreadId);
      toast("Session ID copied to clipboard");
    }
  }

  const loadThreadsCoalesced = coalesce(async () => {
    try {
      threads = await api.listThreads();
    } catch {
      threads = [];
    }
  }, 100);

  async function loadThreads() {
    return loadThreadsCoalesced();
  }

  async function loadProjects() {
    try {
      projects = await api.listProjects();
    } catch {
      projects = [];
    }
  }

  async function refreshEvents() {
    if (!activeThreadId) return;
    const my = ++dataSeq;
    const id = activeThreadId;
    try {
      const [meta, ev] = await api.getThread(id);
      if (my !== dataSeq || activeThreadId !== id) return;
      activeMeta = meta;
      events = ev;
    } catch {}
  }

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
    liveTimer = window.setTimeout(() => requestAnimationFrame(flushLive), 60);
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
      approvals = [
        ...approvals.filter((a) => a.key !== e.key),
        { key: e.key, session: e.session, call: e.call },
      ];
      return;
    }
    if (e.kind === "subsession_created") {
      loadThreads();
      return;
    }
    if (e.kind === "tool_call") {
      if (e.session === activeThreadId && !liveTools.some((t) => t.id === e.id)) {
        liveTools = [...liveTools, { id: e.id, name: e.name, label: e.label || e.name, running: true, ok: true, ms: 0 }];
      }
    } else if (e.kind === "tool_result") {
      if (e.session === activeThreadId) {
        liveTools = liveTools.map((t) => {
          if (t.running && (t.id === e.id || t.name === e.name)) {
            return { ...t, running: false, ok: e.ok, ms: e.ms };
          }
          return t;
        });
      }
    } else if (e.kind === "done" || e.kind === "error") {
      if (liveRun === e.session) {
        liveRun = null;
      }
      approvals = approvals.filter((a) => a.session !== e.session);
    }

    if (e.session !== activeThreadId) return;

    if (e.kind === "text") {
      pushLive(e.text, e.session);
    } else if (e.kind === "reasoning") {
      liveReasoning += e.text;
    } else if (e.kind === "usage") {
      liveTokens = e.tokens_in + e.tokens_out;
      liveCost = e.cost_usd;
    } else if (e.kind === "done" || e.kind === "error") {
      flushLive();
      clearLive();
      void refreshEvents();
    }
  }

  function onGlobalKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === "p" || e.key.toLowerCase() === "k")) {
      e.preventDefault();
      showSessionPopup = !showSessionPopup;
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "t") {
      e.preventDefault();
      handleNewTab();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "w") {
      e.preventDefault();
      handleCloseTab(activeTabId);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === ",") {
      e.preventDefault();
      showSettings = !showSettings;
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "Tab") {
      e.preventDefault();
      const idx = tabs.findIndex((t) => t.id === activeTabId);
      const nextIdx = e.shiftKey
        ? (idx - 1 + tabs.length) % tabs.length
        : (idx + 1) % tabs.length;
      handleSelectTab(tabs[nextIdx].id);
      return;
    }
    if (e.key === "Escape") {
      if (showSessionPopup) showSessionPopup = false;
      else if (sessionMenuOpen) sessionMenuOpen = false;
      else if (showSettings) showSettings = false;
      else if (liveRun) stopRun();
    }
  }

  let hasUpdate = false;

  onMount(() => {
    const unlistenPromise = onRunEvent(onEvent);
    document.addEventListener("parzi:bg", (e) => {
      bg = (e as CustomEvent<string>).detail;
    });

    (async () => {
      try {
        applyThemeCss(await api.getThemeCss());
        bg = await api.backgroundUrl();
        await ensureBoard();
        await loadProjects();
        await loadThreads();
        await loadWorkspaces();
        checkForUpdatesSoon(2000);
      } catch (e) {
        toast(`Startup warning: ${e}`, true);
      }
    })();

    function onDocClick(e: MouseEvent) {
      if (!(e.target as HTMLElement).closest(".session-actions-menu")) {
        sessionMenuOpen = false;
      }
    }
    document.addEventListener("click", onDocClick);

    return () => {
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      document.removeEventListener("click", onDocClick);
    };
  });
</script>

<div class="parzi-app-shell">
  <!-- Dynamic Art Backdrop (Smooth fade on query/navigation) -->
  <DefaultArt {bg} blurred={isBlurredStage} />

  <!-- Integrated TopBar: Menu, Home, Tabs, Spotlight Search, Window Controls -->
  <TopBar
    {tabs}
    {activeTabId}
    on:selectTab={(e) => handleSelectTab(e.detail.id)}
    on:closeTab={(e) => handleCloseTab(e.detail.id)}
    on:newTab={handleNewTab}
    on:home={() => handleSelectTab(tabs[0].id)}
    on:openSettings={() => openSettings("general")}
    on:openSessionPopup={() => (showSessionPopup = true)}
  />

  <div class="app-body">
    <!-- Main Full-width Stage (Zero Sidebar) -->
    <main class="stage-container">
      {#if showSettings}
        <div class="settings-overlay" in:fly={{ y: 8, ...smooth }} out:fly={{ y: 8, ...smoothFast }}>
          <div class="settings-nav-strip">
            <div class="settings-tab-buttons">
              {#each ["general", "providers", "appearance", "connectors", "tools", "skills", "context", "system"] as sec}
                <button
                  class="sec-tab"
                  class:active={settingsSection === sec}
                  on:click={() => (settingsSection = sec)}
                >
                  {sec.charAt(0).toUpperCase() + sec.slice(1)}
                </button>
              {/each}
            </div>
            <button class="settings-close-btn" title="Close Settings (Esc)" on:click={() => (showSettings = false)}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6L6 18M6 6l12 12" /></svg>
            </button>
          </div>
          <div class="settings-body-scroll">
            <Settings settingsTab={settingsSection} bareSection={settingsSection} currentProject={curProject} />
          </div>
        </div>
      {:else if currentTab?.kind === "browser"}
        <div class="browser-tab-stage" in:fade={{ duration: 150 }}>
          <BrowserView
            url={currentTab.url || ""}
            title={currentTab.title}
            on:navigate={(e) => {
              if (currentTab) {
                currentTab.url = e.detail.url;
                currentTab.title = e.detail.url.replace(/^https?:\/\/(www\.)?/, "").slice(0, 24);
                tabs = [...tabs];
              }
            }}
            on:openExternal={(e) => api.openExternalUrl(e.detail.url).catch(() => window.open(e.detail.url, "_blank"))}
          />
        </div>
      {:else}
        <!-- Agent Harness Session View -->
        {#if activeThreadId}
          <div class="session-stage-wrapper">
            <!-- OpenCode Session Subheader (Inside rounded canvas) -->
            <div class="session-subheader">
              <div class="sub-left">
                <h2 class="sub-title">{activeMeta?.title || "New session"}</h2>
                {#if branch}<span class="sub-branch">⎇ {branch}</span>{/if}
              </div>
              <div class="sub-right">
                {#if liveRun}
                  <span class="live-dot" title="Generating…" />
                {/if}
                <button class="sub-circle-btn" title="Status">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
                    <circle cx="12" cy="12" r="9" />
                  </svg>
                </button>
                <div class="session-actions-menu">
                  <button class="sub-menu-btn" title="More options" on:click={toggleSessionMenu}>
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                      <circle cx="5" cy="12" r="2" />
                      <circle cx="12" cy="12" r="2" />
                      <circle cx="19" cy="12" r="2" />
                    </svg>
                  </button>
                  {#if sessionMenuOpen}
                    <div class="sub-dropdown" role="menu">
                      <button class="menu-item" on:click={handleRenameSession}>
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>
                        <span>Rename session</span>
                      </button>
                      <button class="menu-item" on:click={() => { sessionMenuOpen = false; if (activeThreadId) fork(activeThreadId); }}>
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M6 9v6"/><circle cx="18" cy="9" r="3"/><path d="M6 9a9 9 0 0 1 9 9"/></svg>
                        <span>Fork session</span>
                      </button>
                      <button class="menu-item" on:click={handleCopyId}>
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                        <span>Copy Session ID</span>
                      </button>
                      <div class="menu-sep" />
                      <button class="menu-item danger" on:click={() => { sessionMenuOpen = false; if (activeThreadId) handleDeleteThread(activeThreadId); }}>
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                        <span>Delete session</span>
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            </div>

            <!-- Message Stream -->
            <div class="stage-scroll" bind:this={scrollEl} on:scroll={onStageScroll} in:fade={{ duration: 180 }}>
              <Thread
                {events}
                liveText={live}
                liveReasoning={liveReasoning}
                {liveTools}
                approval={activeApproval}
                streaming={!!liveRun}
                projectRoot={currentRoot}
                on:voted={(e) => { approvals = approvals.filter((a) => a.key !== e.detail.key); }}
              />
            </div>

            {#if showScrollBottom}
              <button class="scroll-bottom-btn" title="Scroll to bottom" on:click={scrollToBottom}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12l7 7 7-7"/></svg>
              </button>
            {/if}
          </div>
        {:else}
          <!-- Home / Empty Draft Stage with Hero Art -->
          <div class="home-hero-stage"></div>
        {/if}

        <!-- Floating Omnibar (Transitions smoothly between Hero Center and Bottom Dock) -->
        <div class="omnibar-slot" class:hero={!activeThreadId && !events.length} class:dock={!!activeThreadId || events.length > 0}>
          <Omnibar
            bind:input
            bind:model
            bind:effort
            bind:permission
            bind:attachments
            bind:mode={omniMode}
            streaming={!!liveRun}
            currentProject={curProject}
            projectRoot={currentRoot}
            {branch}
            tokens={liveTokens}
            {contextUsed}
            {contextLimit}
            {compacting}
            board={$board}
            on:send={send}
            on:browse={handleOmniBrowse}
            on:stop={stopRun}
            on:modelChange={(e) => (model = e.detail.model)}
            workspaces={wsNames}
            workspace={curWorkspace}
            workspaceFixed={!!activeThreadId}
            on:workspaceChange={(e) => selectWorkspace(e.detail.workspace)}
            on:error={(e) => toast(e.detail.text, true)}
          />
        </div>
      {/if}
    </main>
  </div>

  <!-- Session Switcher / Spotlight Modal (Zero-sidebar manager) -->
  <SessionPopup
    open={showSessionPopup}
    {threads}
    {tabs}
    {activeTabId}
    workspaces={wsNames}
    currentWorkspace={curWorkspace}
    on:close={() => (showSessionPopup = false)}
    on:selectTab={(e) => handleSelectTab(e.detail.id)}
    on:openSession={(e) => handleOpenSession(e.detail.id)}
    on:deleteSession={(e) => handleDeleteThread(e.detail.id)}
    on:selectWorkspace={(e) => handleSelectWorkspace(e.detail.name)}
    on:newSession={handleNewTab}
    on:newBrowserTab={() => handleNewBrowserTab()}
    on:openSettings={() => openSettings("general")}
  />

  <!-- Toast Stack -->
  <div class="toast-stack">
    {#each toasts as t (t.id)}
      <div class="toast-item" class:error-toast={t.err} transition:fade|local={{ duration: 140 }}>
        {t.text}
      </div>
    {/each}
  </div>
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
  .app-body {
    flex: 1;
    display: flex;
    position: relative;
    z-index: 1;
    overflow: hidden;
  }
  .stage-container {
    flex: 1;
    display: flex;
    flex-direction: column;
    position: relative;
    overflow: hidden;
    width: 100%;
    height: 100%;
  }
  .session-stage-wrapper {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    position: relative;
    background: #18181b;
    border: 1px solid #27272a;
    border-bottom: none;
    border-top-left-radius: 12px;
    border-top-right-radius: 12px;
    margin: 8px 8px 0 8px;
    overflow: hidden;
    animation: stageEnter 450ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  @keyframes stageEnter {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  .session-subheader {
    height: 42px;
    padding: 0 20px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid #27272a;
    background: #18181b;
    user-select: none;
    z-index: 10;
  }
  .sub-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sub-title {
    font-size: 14.5px;
    font-weight: 600;
    color: #ffffff;
    margin: 0;
    letter-spacing: -0.2px;
  }
  .sub-branch {
    font-size: 11px;
    color: #71717a;
  }
  .sub-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sub-circle-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: #71717a;
    cursor: pointer;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .sub-circle-btn:hover {
    background: #27272a;
    color: #f4f4f5;
  }
  .live-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #10b981;
    box-shadow: 0 0 6px #10b981;
    animation: pulse 1.2s ease infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.85); }
  }
  .session-actions-menu {
    position: relative;
  }
  .sub-menu-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: #71717a;
    cursor: pointer;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .sub-menu-btn:hover {
    background: #27272a;
    color: #f4f4f5;
  }
  .sub-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    width: 170px;
    background: #141416;
    border: 1px solid #27272a;
    border-radius: 8px;
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.6);
    padding: 4px;
    z-index: 500;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .stage-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding-bottom: 90px;
    scrollbar-width: thin;
    scrollbar-color: #27272a transparent;
  }
  .stage-scroll::-webkit-scrollbar {
    width: 6px;
  }
  .stage-scroll::-webkit-scrollbar-track {
    background: transparent;
  }
  .stage-scroll::-webkit-scrollbar-thumb {
    background: #27272a;
    border-radius: 999px;
  }
  .stage-scroll::-webkit-scrollbar-thumb:hover {
    background: #3f3f46;
  }
  .scroll-bottom-btn {
    position: absolute;
    bottom: 96px;
    left: 50%;
    transform: translateX(-50%);
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: #18181b;
    border: 1px solid #27272a;
    color: #a1a1aa;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    z-index: 15;
    transition: background 0.1s ease, color 0.1s ease, transform 0.1s ease;
  }
  .scroll-bottom-btn:hover {
    background: #27272a;
    color: #ffffff;
    transform: translateX(-50%) scale(1.06);
  }
  .home-hero-stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }
  .browser-tab-stage {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top-left-radius: 12px;
    border-top-right-radius: 12px;
    margin: 8px 8px 0 8px;
    overflow: hidden;
    border: 1px solid #27272a;
    border-bottom: none;
  }
  .omnibar-slot {
    position: absolute;
    left: 50%;
    z-index: 20;
    display: flex;
    justify-content: center;
    width: 100%;
    pointer-events: none;
    transition:
      bottom 700ms cubic-bezier(0.16, 1, 0.3, 1),
      transform 700ms cubic-bezier(0.16, 1, 0.3, 1),
      width 500ms ease;
  }
  .omnibar-slot :global(.ob) {
    pointer-events: auto;
  }
  .omnibar-slot.hero {
    bottom: 46%;
    transform: translate(-50%, 50%);
    width: min(720px, 90%);
    padding: 0;
  }
  .omnibar-slot.dock {
    bottom: 14px;
    transform: translate(-50%, 0);
    width: min(740px, calc(100% - 40px));
    padding: 0;
  }
  .settings-overlay {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--stage, #0c0d12);
    z-index: 30;
  }
  .settings-nav-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 16px;
    border-bottom: 1px solid var(--line-3, rgba(255, 255, 255, 0.08));
    background: rgba(16, 18, 24, 0.6);
  }
  .settings-tab-buttons {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow-x: auto;
  }
  .sec-tab {
    padding: 5px 11px;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--text-3);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .sec-tab:hover {
    background: var(--surface-2, rgba(255, 255, 255, 0.06));
    color: var(--text);
  }
  .sec-tab.active {
    background: var(--surface-3, rgba(255, 255, 255, 0.12));
    color: var(--text, #ffffff);
    font-weight: 500;
  }
  .settings-close-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-3);
    cursor: pointer;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .settings-close-btn:hover {
    background: var(--surface-2, rgba(255, 255, 255, 0.1));
    color: var(--text);
  }
  .settings-body-scroll {
    flex: 1;
    overflow-y: auto;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 5px;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    width: 100%;
    box-sizing: border-box;
    transition: background 0.1s ease;
  }
  .menu-item:hover {
    background: var(--surface-2, rgba(255, 255, 255, 0.08));
  }
  .menu-item.danger:hover {
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
  }
  .mi-icon {
    font-size: 12px;
  }
  .menu-sep {
    height: 1px;
    background: var(--line-3, rgba(255, 255, 255, 0.06));
    margin: 3px 0;
  }
  .toast-stack {
    position: fixed;
    bottom: 16px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 1200;
    pointer-events: none;
  }
  .toast-item {
    background: var(--menu, #14151a);
    backdrop-filter: blur(14px) saturate(1.2);
    -webkit-backdrop-filter: blur(14px) saturate(1.2);
    border: 1px solid var(--line-3, rgba(255, 255, 255, 0.1));
    color: var(--text);
    font-size: 12px;
    padding: 8px 14px;
    border-radius: 6px;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
  }
  .toast-item.error-toast {
    border-color: #ef4444;
    color: #ef4444;
  }
</style>
