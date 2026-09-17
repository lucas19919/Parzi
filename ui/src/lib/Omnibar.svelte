<script lang="ts">
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { createEventDispatcher } from "svelte";
  import { scale, fade, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { updateProviderRow } from "./modelStore";
  import type { ModelRow } from "./api";

  export let input = "";
  export let model = "";
  export let effort: "low" | "medium" | "high" | "extra" | "ultra" = "medium";
  export let streaming = false;
  export let currentProject = "default";
  export let currentTask: string | null = null;
  export let currentSubfolder: string | null = null;
  export let branch = "";
  export let tokens = 0;
  export let models: ModelRow[] = [];
  export let projectRoot = "";
  export let attachments: string[] = [];
  export let mode: "chat" | "plan" | "build" = "chat";
  export let permission: string = "full";
  /** Workspaces a new chat can belong to. Empty list hides the picker. */
  export let workspaces: string[] = [];
  /** Workspace this chat belongs to ("" = none). */
  export let workspace = "";
  /** Sent chats keep their workspace: the pill shows it but does not open. */
  export let workspaceLocked = false;
  /** Context window fill of this chat (tokens the model saw last request). */
  export let contextUsed = 0;
  /** The model's window; 0 hides the meter. */
  export let contextLimit = 0;
  /** A compaction is running: the meter spins and does not click. */
  export let compacting = false;

  $: contextPct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;
  const RING = 2 * Math.PI * 6;
  function kTokens(n: number): string {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
    return String(n);
  }

  const dispatch = createEventDispatcher<{
    send: void;
    stop: void;
    modelChange: { model: string };
    command: { name: string; arg: string };
    openPlanner: void;
    permissionChange: { permission: string };
    workspaceChange: { workspace: string };
    newWorkspace: void;
  }>();

  const I = {
    plus: "M12 5v14M5 12h14",
    sendUp: "M12 19V5M5 12l7-7 7 7",
    stop: "M7 7h10v10H7z",
    chevD: "M6 9l6 6 6-6",
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    check: "M20 6L9 17l-5-5",
    lock: "M7 11V7a5 5 0 0 1 10 0v4M5 11h14v10H5zM12 15v3",
    unlock: "M7 11V7a5 5 0 0 1 9.9-1M5 11h14v10H5zM12 15v3",
    pencil: "M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    spark: "M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4Z",
    clip: "M21 11.5l-8.5 8.5a5.5 5.5 0 0 1-7.8-7.8l8-8a3.7 3.7 0 0 1 5.2 5.2l-8 8a1.8 1.8 0 0 1-2.6-2.6l7-7",
    model: "M4 4h16v16H4z",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    file: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6",
    close: "M18 6L6 18M6 6l12 12",
  };

  const PERMS = [
    { id: "supervised", title: "Supervised", desc: "Ask before commands and file changes.", icon: I.lock },
    { id: "edits", title: "Auto-accept edits", desc: "Auto-approve edits, ask before other actions.", icon: I.pencil },
    { id: "auto", title: "Auto", desc: "Supported providers approve routine actions; others still ask.", icon: I.spark },
    { id: "full", title: "Full access", desc: "Allow commands and edits without prompts.", icon: I.unlock },
  ];

  const MODES: { id: "chat" | "plan" | "build"; title: string; desc: string }[] = [
    { id: "chat", title: "Chat", desc: "Ask and edit freely." },
    { id: "plan", title: "Plan", desc: "Plan only — no edits or commands." },
    { id: "build", title: "Build", desc: "Ship it — full execution." },
  ];

  let textareaEl: HTMLTextAreaElement | null = null;
  let showModelPicker = false;
  /** Left-rail selection: a roster provider id, or "__fav" for starred. */
  let railSel = "";
  /** Provider with a live catalog pull in flight (cached rows stay visible). */
  let refreshingProvider: string | null = null;
  const refreshedAt = new Map<string, number>();
  let effortOpen = false;
  let permOpen = false;
  let wsOpen = false;
  let modelQuery = "";
  let searchInputEl: HTMLInputElement | null = null;
  let modelIndex = 0;
  let modelBtn: HTMLButtonElement | null = null;
  let effortBtn: HTMLButtonElement | null = null;
  let permBtn: HTMLButtonElement | null = null;
  let wsBtn: HTMLButtonElement | null = null;
  let wsPopStyle = "";
  let wsBelow = false;
  let modelPopStyle = "";
  let effortPopStyle = "";
  let permPopStyle = "";
  let modelBelow = false;
  let effortBelow = false;
  let permBelow = false;
  let atOpen = false;
  let atItems: string[] = [];
  let atIndex = 0;
  let slashOpen = false;
  let slashIndex = 0;

  /** Roster, in picker order. Mirrors the backend `PROVIDERS` list. */
  const PROVIDER_ORDER = ["claude", "codex", "antigravity", "opencode", "xai"];
  const PROVIDER_NAME: Record<string, string> = {
    claude: "Claude",
    codex: "Codex",
    antigravity: "Antigravity",
    opencode: "OpenCode",
    xai: "Grok",
  };


  interface FamRow {
    provider: string;
    auth: string;
    billing: string;
    family: string;
    label: string;
    value: string;
    legacy: boolean;
  }

  /** Collapsed families: one row per (provider, family), variant follows effort. */
  $: allFams = (() => {
    const seen = new Map<string, FamRow>();
    for (const r of models) {
      for (const m of r.models) {
        const family = m.family || m.id;
        const key = `${r.provider}/${family}`;
        if (!seen.has(key)) {
          seen.set(key, {
            provider: r.provider,
            auth: r.auth,
            billing: r.billing ?? "none",
            family,
            label: m.family_name || m.name || m.id,
            value: `${r.provider}/${family}`,
            legacy: !!m.legacy,
          });
        }
      }
    }
    return [...seen.values()];
  })();

  $: q = modelQuery.trim().toLowerCase();

  function matches(row: FamRow): boolean {
    if (!q) return true;
    return (
      row.label.toLowerCase().includes(q) ||
      row.provider.toLowerCase().includes(q) ||
      (PROVIDER_NAME[row.provider] ?? "").toLowerCase().includes(q) ||
      row.family.toLowerCase().includes(q)
    );
  }

  /** Short status for a provider header. */
  function statusOf(r: ModelRow): { label: string; kind: "sub" | "key" | "off" | "expired" } {
    if (r.auth === "expired") return { label: "Expired", kind: "expired" };
    if (r.auth !== "ok") return { label: "Sign in", kind: "off" };
    if (r.billing === "subscription") return { label: r.account || "Subscription", kind: "sub" };
    return { label: "API key", kind: "key" };
  }

  /** Synthetic picker row: Smart Auto routes across signed-in providers. */
  const AUTO_ROW: FamRow = {
    provider: "auto",
    auth: "ok",
    billing: "none",
    family: "auto",
    label: "Smart Auto",
    value: "auto",
    legacy: false,
  };

  /**
   * Side-by-side picker (T3-style): the left rail switches providers, the
   * right pane lists models. Nothing ever moves under the cursor, so a
   * click can never leak through a step transition into a model pick.
   * Searching overrides the rail and matches across all providers.
   */
  $: mainItems = ((): FamRow[] => {
    if (q) return [AUTO_ROW, ...allFams].filter(matches);
    if (railSel === "__auto") return [AUTO_ROW];
    if (railSel === "__fav") return [...favRows];
    if (!railSel) return [];
    return allFams.filter((r) => r.provider === railSel);
  })();
  $: if (modelIndex >= mainItems.length && mainItems.length) modelIndex = 0;

  $: activeModelInfo = (() => {
    for (const r of models) {
      for (const m of r.models) {
        if (`${r.provider}/${m.id}` === model || m.id === model) {
          return { provider: r.provider, name: m.name || m.id, auth: r.auth };
        }
      }
    }
    if (model === "auto") return { provider: "auto", name: "Smart Auto", auth: "ok" };
    const [p, ...rest] = model.split("/");
    return { provider: p || "other", name: rest.join("/") || model || "Select model", auth: "ok" };
  })();

  $: activeFamilyLabel = (() => {
    const [p, ...rest] = model.split("/");
    const fam = rest.join("/");
    for (const r of models) {
      if (r.provider !== p) continue;
      for (const m of r.models) {
        if (m.id === fam || (m.family && `${m.family}` === fam)) {
          return { name: m.family_name || m.name || m.id, tier: "" };
        }
      }
    }
    return { name: activeModelInfo.name, tier: "" };
  })();

  interface EffortOpt {
    id: string;
    label: string;
    hint: string;
  }

  let favorites: string[] = [];
  let effortOpts: EffortOpt[] = [
    { id: "low", label: "Low", hint: "4k output" },
    { id: "medium", label: "Medium", hint: "16k output" },
    { id: "high", label: "High", hint: "64k output" },
    { id: "extra", label: "Extra", hint: "128k output" },
    { id: "ultra", label: "Ultra", hint: "256k output" },
  ];
  const effortCache = new Map<string, EffortOpt[]>();

  $: favRows = favorites
    .map((v) => allFams.find((r) => r.value === v))
    .filter((r): r is FamRow => !!r);
  $: effortLabel = effortOpts.find((o) => o.id === effort)?.label ?? effort;
  $: modeLabel = mode === "chat" ? "Chat" : mode === "plan" ? "Plan" : "Build";
  $: permTitle = PERMS.find((p) => p.id === permission)?.title ?? "Full access";

  $: variantFor = (row: FamRow) => {
    const fam = row.family;
    if (fam.startsWith("gemini-3.8-flash") || fam.startsWith("gemini-3.7-flash") || fam.startsWith("gemini-3.6-flash")) {
      // Only low/medium/high variants exist: upper rungs ride high.
      return effort === "low" ? "low" : effort === "medium" ? "medium" : "high";
    }
    if (fam === "gemini-3.1-pro") return effort === "low" ? "low" : "high";
    return "";
  };

  /** Second line of a picker row: provider · plan, plus variant/legacy. */
  function planSub(row: FamRow): string {
    if (row.provider === "auto") return "Routes across your signed-in providers";
    const parts: string[] = [PROVIDER_NAME[row.provider] ?? row.provider];
    const pr = models.find((r) => r.provider === row.provider);
    const plan =
      pr?.account ||
      (pr?.billing === "subscription"
        ? "Subscription"
        : pr?.billing === "api_key"
          ? "API key"
          : "");
    if (plan) parts.push(plan);
    const v = variantFor(row);
    if (v) parts.push(`${v} variant`);
    if (row.legacy) parts.push("legacy");
    return parts.join(" · ");
  }

  async function refreshEffort(provider: string) {
    if (effortCache.has(provider)) {
      applyEffortOpts(effortCache.get(provider)!);
      return;
    }
    try {
      const opts = await api.effortOptions(provider);
      effortCache.set(provider, opts);
      applyEffortOpts(opts);
    } catch {
      /* keep defaults offline */
    }
  }

  function applyEffortOpts(opts: EffortOpt[]) {
    if (!opts.length) return;
    effortOpts = opts;
    if (!opts.some((o) => o.id === effort)) {
      setEffort(opts[0].id);
    }
  }

  type EffortId = "low" | "medium" | "high" | "extra" | "ultra";
  const EFFORT_IDS: EffortId[] = ["low", "medium", "high", "extra", "ultra"];

  function setEffort(id: string) {
    // Legacy "med" still resolves (old defaults, queued runs).
    const norm = id === "med" ? "medium" : id;
    if ((EFFORT_IDS as string[]).includes(norm)) effort = norm as typeof effort;
  }

  function setMode(m: "chat" | "plan" | "build") {
    mode = m;
    effortOpen = false;
  }

  function setPermission(id: string) {
    permission = id;
    permOpen = false;
    dispatch("permissionChange", { permission: id });
  }

  async function toggleFav(value: string) {
    try {
      favorites = await api.toggleFavorite(value);
    } catch {}
  }
  function selectModel(id: string) {
    model = id;
    showModelPicker = false;
    refreshEffort(id.split("/")[0]);
    dispatch("modelChange", { model: id });
  }

  function pickFamily(row: FamRow) {
    // Keyless `opencode serve` answers on loopback without credentials, so
    // its models stay pickable: a dead server fails honestly at send time.
    const openServe = row.provider === "opencode";
    if (row.auth !== "ok" && !openServe) {
      dispatch("command", { name: "keys", arg: row.provider });
      showModelPicker = false;
      return;
    }
    selectModel(row.value);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" && (e.target as HTMLInputElement)?.placeholder?.includes("Search models")) {
        return; // let model search input handle its own Enter
      }
      e.preventDefault();
      if (slashOpen && slashItems.length) {
        runSlash(slashItems[slashIndex] ?? slashItems[0]);
      } else if (atOpen && atItems.length) {
        pickAt(atItems[atIndex] ?? atItems[0]);
      } else if (!streaming && input.trim()) {
        dispatch("send");
      }
    } else if (e.key === "Escape") {
      if (showModelPicker) showModelPicker = false;
      else if (effortOpen) effortOpen = false;
      else if (permOpen) permOpen = false;
      else if (wsOpen) wsOpen = false;
      else if (slashOpen) slashOpen = false;
      else if (atOpen) atOpen = false;
    } else if (slashOpen && e.key === "ArrowDown") {
      e.preventDefault();
      slashIndex = (slashIndex + 1) % Math.max(1, slashItems.length);
    } else if (slashOpen && e.key === "ArrowUp") {
      e.preventDefault();
      slashIndex = (slashIndex - 1 + Math.max(1, slashItems.length)) % Math.max(1, slashItems.length);
    } else if (atOpen && e.key === "ArrowDown") {
      e.preventDefault();
      atIndex = (atIndex + 1) % atItems.length;
    } else if (atOpen && e.key === "ArrowUp") {
      e.preventDefault();
      atIndex = (atIndex - 1 + atItems.length) % atItems.length;
    }
  }

  async function handleInput() {
    if (textareaEl) {
      textareaEl.style.height = "auto";
      textareaEl.style.height = Math.min(textareaEl.scrollHeight, 180) + "px";
    }
    const at = input.match(/@([\w./-]*)$/);
    if (at && projectRoot) {
      try {
        atItems = await api.listFiles(projectRoot, at[1].split("/").slice(-1)[0]);
        atIndex = 0;
        atOpen = atItems.length > 0;
      } catch {
        atOpen = false;
      }
    } else {
      atOpen = false;
    }
    slashOpen = /^\/\w*$/.test(input.trim());
    slashIndex = 0;
  }

  function pickAt(item: string) {
    input = input.replace(/@[\w./-]*$/, `@${item} `);
    if (!attachments.includes(item)) attachments = [...attachments, item];
    atOpen = false;
    textareaEl?.focus();
    handleInput();
  }

  interface SlashCmd {
    name: string;
    hint: string;
    run: () => void;
  }

  $: slashItems = (() => {
    const q = input.trim().slice(1).toLowerCase();
    const all: SlashCmd[] = [
      { name: "/plan", hint: "task planner", run: () => dispatch("command", { name: "plan", arg: "" }) },
      { name: "/auto", hint: "smart auto routing", run: () => dispatch("command", { name: "auto", arg: "" }) },
      { name: "/model", hint: "open model picker", run: () => { openModelPicker(); } },
      { name: "/effort", hint: "cycle effort", run: () => dispatch("command", { name: "effort", arg: "" }) },
      { name: "/new", hint: "new thread", run: () => dispatch("command", { name: "new", arg: "" }) },
      { name: "/clear", hint: "clean thread, same lane", run: () => dispatch("command", { name: "clear", arg: "" }) },
      { name: "/compact", hint: "summarize to free context", run: () => dispatch("command", { name: "compact", arg: "" }) },
      { name: "/fork", hint: "branch this thread", run: () => dispatch("command", { name: "fork", arg: "" }) },
      { name: "/subsession", hint: "spawn a child subsession", run: () => dispatch("command", { name: "subsession", arg: "" }) },
      { name: "/kill", hint: "stop the run", run: () => dispatch("command", { name: "kill", arg: "" }) },
      { name: "/doctor", hint: "health checks", run: () => dispatch("command", { name: "doctor", arg: "" }) },
      { name: "/help", hint: "all commands", run: () => dispatch("command", { name: "help", arg: "" }) },
    ];
    return all.filter((c) => c.name.slice(1).startsWith(q));
  })();

  function runSlash(cmd: SlashCmd) {
    input = "";
    slashOpen = false;
    handleInput();
    cmd.run();
  }

  const POP_W = { model: 440, effort: 300, perm: 320, ws: 240 };

  /**
   * Pin a menu above its trigger when there is room — always staying clear
   * of the 38px titlebar drag zone (clicks landing up there hit the window
   * chrome instead: the menu closes and the window drags). Otherwise drop
   * the menu below the trigger.
   */
  function placePop(btn: HTMLButtonElement | null, which: "model" | "effort" | "perm" | "ws") {
    if (!btn || typeof window === "undefined") return;
    const r = btn.getBoundingClientRect();
    const w = POP_W[which];
    const left = Math.max(8, Math.min(which === "perm" || which === "ws" ? r.right - w : r.left, window.innerWidth - w - 8));
    const spaceAbove = r.top - 46;
    const spaceBelow = window.innerHeight - r.bottom - 8;
    let style: string;
    let below: boolean;
    if (spaceAbove >= 300 || spaceAbove >= spaceBelow) {
      const maxH = Math.max(180, Math.min(spaceAbove, 560));
      const bottom = Math.max(8, window.innerHeight - r.top + 8);
      style = `left:${Math.round(left)}px;bottom:${Math.round(bottom)}px;max-height:${Math.round(maxH)}px;`;
      below = false;
    } else {
      const maxH = Math.max(180, spaceBelow);
      style = `left:${Math.round(left)}px;top:${Math.round(r.bottom + 8)}px;max-height:${Math.round(maxH)}px;`;
      below = true;
    }
    if (which === "model") {
      modelPopStyle = style;
      modelBelow = below;
    } else if (which === "effort") {
      effortPopStyle = style;
      effortBelow = below;
    } else if (which === "ws") {
      wsPopStyle = style;
      wsBelow = below;
    } else {
      permPopStyle = style;
      permBelow = below;
    }
  }

  function repositionPops() {
    if (showModelPicker) placePop(modelBtn, "model");
    if (effortOpen) placePop(effortBtn, "effort");
    if (permOpen) placePop(permBtn, "perm");
    if (wsOpen) placePop(wsBtn, "ws");
  }

  function closeInlinePops() {
    slashOpen = false;
    atOpen = false;
  }

  /** Roster provider behind the current model value, if any. */
  function providerOfModel(m: string): string | null {
    const [p] = m.split("/");
    if (m === "auto" || !p) return null;
    return PROVIDER_ORDER.includes(p) && models.some((r) => r.provider === p) ? p : null;
  }

  /** Live catalog for one provider; cached rows stay visible meanwhile. */
  async function refreshProviderRow(p: string) {
    const last = refreshedAt.get(p) ?? 0;
    if (Date.now() - last < 45_000) return;
    refreshedAt.set(p, Date.now());
    refreshingProvider = p;
    try {
      updateProviderRow(await api.refreshProvider(p));
    } catch {
      /* picker never blocks on network */
    } finally {
      if (refreshingProvider === p) refreshingProvider = null;
    }
  }

  /** Rail switch: the model pane updates to the right, cursor stays put. */
  function selectRail(p: string) {
    railSel = p;
    modelQuery = "";
    modelIndex = 0;
    if (p && p !== "__fav") void refreshProviderRow(p);
    setTimeout(() => searchInputEl?.focus(), 30);
  }

  function openModelPicker() {
    showModelPicker = true;
    effortOpen = false;
    permOpen = false;
    closeInlinePops();
    modelQuery = "";
    modelIndex = 0;
    // Start where the user is: Smart Auto, current provider, else the first roster row.
    railSel =
      model === "auto"
        ? "__auto"
        : (providerOfModel(model) ??
          PROVIDER_ORDER.find((p) => models.some((r) => r.provider === p)) ??
          "");
    if (railSel) void refreshProviderRow(railSel);
    placePop(modelBtn, "model");
    api
      .getConfig()
      .then((c) => {
        favorites = c.favorite_models ?? [];
      })
      .catch(() => {});
    refreshEffort(activeModelInfo.provider);
    setTimeout(() => searchInputEl?.focus(), 30);
  }

  function togglePicker() {
    if (showModelPicker) {
      showModelPicker = false;
      return;
    }
    openModelPicker();
  }

  function toggleEffort() {
    effortOpen = !effortOpen;
    if (effortOpen) {
      showModelPicker = false;
      permOpen = false;
      closeInlinePops();
      placePop(effortBtn, "effort");
      refreshEffort(activeModelInfo.provider);
    }
  }

  function toggleWs() {
    if (workspaceLocked) return;
    wsOpen = !wsOpen;
    if (wsOpen) {
      showModelPicker = false;
      effortOpen = false;
      permOpen = false;
      closeInlinePops();
      placePop(wsBtn, "ws");
    }
  }

  function pickWorkspace(name: string) {
    wsOpen = false;
    if (name !== workspace) dispatch("workspaceChange", { workspace: name });
  }

  function togglePerm() {
    permOpen = !permOpen;
    if (permOpen) {
      showModelPicker = false;
      effortOpen = false;
      closeInlinePops();
      placePop(permBtn, "perm");
    }
  }

  function onModelKey(e: KeyboardEvent) {
    if (e.ctrlKey && ["1", "2", "3", "4", "5"].includes(e.key)) {
      e.preventDefault();
      const row = mainItems[Number(e.key) - 1];
      if (row) pickFamily(row);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      modelIndex = (modelIndex + 1) % Math.max(1, mainItems.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      modelIndex = (modelIndex - 1 + Math.max(1, mainItems.length)) % Math.max(1, mainItems.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const row = mainItems[modelIndex];
      if (row) pickFamily(row);
    } else if (e.key === "Escape") {
      showModelPicker = false;
    }
  }

  function onWindowClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    if (showModelPicker && !el.closest(".model-zone") && !el.closest(".pop")) showModelPicker = false;
    if (effortOpen && !el.closest(".effort-zone") && !el.closest(".pop")) effortOpen = false;
    if (permOpen && !el.closest(".perm-zone") && !el.closest(".pop")) permOpen = false;
    if (wsOpen && !el.closest(".ws-zone") && !el.closest(".pop")) wsOpen = false;
    if (slashOpen && !el.closest(".slash-pop")) slashOpen = false;
    if (atOpen && !el.closest(".at-popup")) atOpen = false;
  }

  function shortName(name: string): string {
    const base = name.split("/").pop() ?? name;
    return base.length > 22 ? base.slice(0, 22) + "…" : base;
  }

  const IMG_EXT = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif"]);

  function isImage(name: string): boolean {
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    return IMG_EXT.has(ext);
  }

  /** Data-URL previews via the shell (the asset protocol scope does not
      cover workspace files, so convertFileSrc thumbs stay blank). */
  let thumbUrls: Record<string, string | null> = {};
  let thumbReq = 0;
  $: void preloadThumbs(attachments, projectRoot);
  async function preloadThumbs(list: string[], root: string) {
    const my = ++thumbReq;
    for (const a of list) {
      if (a in thumbUrls || !isImage(a)) continue;
      try {
        const url = await api.readImageDataUrl(a, root);
        if (my !== thumbReq) return;
        thumbUrls = { ...thumbUrls, [a]: url };
      } catch {
        if (my === thumbReq) thumbUrls = { ...thumbUrls, [a]: null };
      }
    }
    if (my === thumbReq) {
      // Drop cache rows for removed attachments so re-adding reloads.
      const keep = new Set(list);
      const next: Record<string, string | null> = {};
      for (const k of Object.keys(thumbUrls)) if (keep.has(k)) next[k] = thumbUrls[k];
      thumbUrls = next;
    }
  }

  function removeAttachment(a: string) {
    attachments = attachments.filter((x) => x !== a);
  }

  function clearAttachments() {
    attachments = [];
    thumbUrls = {};
  }

  let pickingFiles = false;

  function addAttachments(paths: string[]) {
    const root = (projectRoot || "").replace(/[/\\]+$/, "");
    const norm = paths
      .map((p) => {
        if (root && (p === root || p.startsWith(root + "\\") || p.startsWith(root + "/"))) {
          return p.slice(root.length).replace(/^[/\\]+/, "");
        }
        return p;
      })
      .filter((p) => p.length > 0);
    if (!norm.length) return;
    attachments = [...attachments, ...norm.filter((n) => !attachments.includes(n))].slice(0, 8);
  }

  let stageErr = "";

  /** Stage clipboard/dropped image files through the shell and attach the
      returned paths. Text pastes and non-image drops pass through untouched. */
  async function stageImageFiles(files: FileList | File[]): Promise<void> {
    const imgs = [...files].filter(
      (f) => f.type.startsWith("image/") || isImage(f.name)
    );
    if (!imgs.length) return;
    stageErr = "";
    for (const f of imgs.slice(0, Math.max(0, 8 - attachments.length))) {
      try {
        const dataUrl = await new Promise<string>((res, rej) => {
          const r = new FileReader();
          r.onload = () => res(String(r.result ?? ""));
          r.onerror = () => rej(r.error);
          r.readAsDataURL(f);
        });
        const b64 = dataUrl.split(",", 2)[1] ?? "";
        if (!b64) continue;
        addAttachments([await api.stageImage(f.name || "pasted.png", b64)]);
      } catch (e) {
        stageErr = `Couldn't attach ${f.name || "image"}: ${e}`;
      }
    }
  }

  function onPaste(e: ClipboardEvent) {
    const files = e.clipboardData?.files;
    if (!files?.length) return;
    if (![...files].some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void stageImageFiles(files);
  }

  function onDropFiles(e: DragEvent) {
    const files = e.dataTransfer?.files;
    if (!files?.length) return;
    if (![...files].some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void stageImageFiles(files);
  }

  /** Native file explorer. Falls back to @ mention flow outside Tauri (browser dev). */
  async function pickFiles() {
    if (pickingFiles) return;
    pickingFiles = true;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const sel = await open({
        multiple: true,
        directory: false,
        defaultPath: projectRoot || undefined,
      });
      if (!sel) return;
      const list = Array.isArray(sel) ? sel : [sel];
      addAttachments(
        list.map((s) => (typeof s === "string" ? s : (s as { path: string }).path))
      );
    } catch {
      input += "@";
      textareaEl?.focus();
      handleInput();
    } finally {
      pickingFiles = false;
    }
  }
</script>

<svelte:window on:click={onWindowClick} on:keydown={handleKeydown} on:resize={repositionPops} />

<div class="ob">
  <div
    class="ob-card"
    role="group"
    aria-label="Message composer — drop images to attach"
    on:dragover|preventDefault
    on:drop|preventDefault={onDropFiles}
    title={`${currentProject}${currentTask ? ` / ${currentTask}` : ""}${currentSubfolder ? ` (${currentSubfolder})` : ""}${branch ? ` ⎇ ${branch}` : ""}${tokens > 0 ? ` · ${tokens} tok` : ""}`}
  >
    {#if attachments.length}
      <div class="attach-grid" transition:slide={{ duration: 160, easing: cubicOut }}>
        {#each attachments as a (a)}
          {@const src = thumbUrls[a] ?? null}
          <div class="thumb" class:has-img={!!src} title={a} transition:scale={{ duration: 140, start: 0.9, easing: cubicOut }}>
            {#if src}
              <img src={src} alt="" class="thumb-img" draggable="false" />
              <span class="thumb-name over">{shortName(a)}</span>
            {:else}
              <Icon d={I.file} size={16} />
              <span class="thumb-name">{shortName(a)}</span>
            {/if}
            <button class="thumb-x" title="Remove attachment" aria-label="Remove {a}" on:click={() => removeAttachment(a)}>
              <Icon d={I.close} size={10} />
            </button>
          </div>
        {/each}
        {#if attachments.length > 1}
          <button class="attach-clear" title="Remove all attachments" on:click={clearAttachments}>clear all</button>
        {/if}
      </div>
    {/if}

    <textarea
      bind:this={textareaEl}
      rows="1"
      placeholder={streaming ? "Working… Esc to stop" : "Message"}
      bind:value={input}
      on:input={handleInput}
      on:paste={onPaste}
    />
    {#if stageErr}<div class="stage-err" role="alert">{stageErr}</div>{/if}

    {#if slashOpen && slashItems.length}
      <div class="slash-pop" transition:scale={{ duration: 150, start: 0.96, easing: cubicOut }}>
        {#each slashItems as cmd, i}
          <button class:on={i === slashIndex} on:click={() => runSlash(cmd)} on:mousemove={() => (slashIndex = i)}>
            <span class="n">{cmd.name}</span><span class="h">{cmd.hint}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if atOpen}
      <div class="at-popup" transition:fade={{ duration: 120 }}>
        {#each atItems.slice(0, 8) as item, i}
          <button class:on={i === atIndex} on:click={() => pickAt(item)}>{item}</button>
        {/each}
      </div>
    {/if}

    <div class="controls">
      <div class="model-zone ctl-zone">
        <button bind:this={modelBtn} class="ctl" class:open={showModelPicker} on:click|stopPropagation={togglePicker} title="Model — open picker">
          {#if hasMark(activeModelInfo.provider)}
            <ProviderLogo provider={activeModelInfo.provider} size={13} />
          {:else}
            <span class="ctl-glyph"><Icon d={I.model} size={13} /></span>
          {/if}
          <span class="truncate">{activeFamilyLabel.name}</span>
          <span class="chev"><Icon d={I.chevD} size={11} /></span>
        </button>
      </div>

      <span class="vdiv" />

      <div class="effort-zone ctl-zone">
        <button bind:this={effortBtn} class="ctl" class:open={effortOpen} on:click|stopPropagation={toggleEffort} title="Effort and mode">
          <span class="truncate">{effortLabel} · {modeLabel}</span>
          <span class="chev"><Icon d={I.chevD} size={11} /></span>
        </button>
      </div>

      <span class="vdiv" />

      <div class="perm-zone ctl-zone">
        <button bind:this={permBtn} class="ctl" class:open={permOpen} on:click|stopPropagation={togglePerm} title="Permission policy">
          <span class="ctl-glyph"><Icon d={I.lock} size={13} /></span>
          <span class="truncate">{permTitle}</span>
          <span class="chev"><Icon d={I.chevD} size={11} /></span>
        </button>
      </div>

      {#if workspaces.length || workspace}
        <span class="vdiv" />
        <div class="ws-zone ctl-zone">
          <button bind:this={wsBtn} class="ctl" class:open={wsOpen} class:locked={workspaceLocked} on:click|stopPropagation={toggleWs}
            title={workspaceLocked ? `This chat belongs to ${workspace || "no workspace"}` : "Workspace for this chat"}>
            <span class="ctl-glyph"><Icon d={I.folder} size={13} /></span>
            <span class="truncate">{workspace || "No workspace"}</span>
            {#if !workspaceLocked}<span class="chev"><Icon d={I.chevD} size={11} /></span>{/if}
          </button>
        </div>
      {/if}

      <span class="spacer" />

      {#if contextLimit > 0 && (contextUsed > 0 || compacting)}
        <button
          class="ctx"
          class:warn={contextPct >= 70}
          class:bad={contextPct >= 90}
          class:busy={compacting}
          disabled={compacting || streaming}
          title={compacting
            ? "Compacting the conversation…"
            : `Context ${contextPct}% full: ${kTokens(contextUsed)} of ${kTokens(contextLimit)} tokens.\nClick to compact; it also happens on its own near the limit.`}
          on:click={() => dispatch("command", { name: "compact", arg: "" })}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle class="ctx-track" cx="8" cy="8" r="6" />
            <circle
              class="ctx-fill"
              cx="8"
              cy="8"
              r="6"
              stroke-dasharray={RING}
              stroke-dashoffset={compacting ? RING * 0.7 : RING * (1 - contextPct / 100)}
            />
          </svg>
          <span>{contextPct}%</span>
        </button>
      {/if}

      <button class="icon-btn" title="Attach files" on:click={pickFiles}>
        <Icon d={I.clip} size={15} />
      </button>
      {#if streaming}
        <button class="go stop" title="Stop run" on:click={() => dispatch("stop")}>
          <Icon d={I.stop} size={11} />
        </button>
      {:else}
        <button class="go" class:ready={!!input.trim()} title="Send (Enter)" disabled={!input.trim()} on:click={() => dispatch("send")}>
          <Icon d={I.sendUp} size={14} />
        </button>
      {/if}
    </div>
  </div>

    {#if showModelPicker}
      <div class="pop model-pop from-left" class:from-top={modelBelow} style={modelPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        <div class="pop-search">
          <Icon d={I.search} size={13} />
          <input
            bind:this={searchInputEl}
            placeholder={q ? "Search all models..." : railSel === "__fav" ? "Starred models" : railSel === "__auto" ? "Smart Auto routing" : `Search ${(PROVIDER_NAME[railSel] ?? railSel)} models...`}
            bind:value={modelQuery}
            on:keydown={onModelKey}
          />
          {#if refreshingProvider && refreshingProvider === railSel}
            <span class="spin" title="Refreshing live catalog" />
          {/if}
        </div>
        <div class="pick-body">
          <div class="prov-rail">
            <button
              class="rail-row"
              class:on={railSel === "__auto"}
              on:click={() => selectRail("__auto")}
              title="Smart Auto — route across providers"
            >
              <span class="rail-auto"><Icon d={I.spark} size={15} /></span>
            </button>
            {#if favRows.length}
              <button
                class="rail-row"
                class:on={railSel === "__fav"}
                on:click={() => selectRail("__fav")}
                title="Starred models"
              >
                <span class="rail-star">★</span>
              </button>
            {/if}
            {#each PROVIDER_ORDER as p}
              {@const pr = models.find((r) => r.provider === p)}
              {#if pr}
                {@const st = statusOf(pr)}
                <button
                  class="rail-row"
                  class:on={railSel === p}
                  class:dim={st.kind === "off" || st.kind === "expired"}
                  on:click={() => selectRail(p)}
                  title={st.kind === "off" || st.kind === "expired" ? `${PROVIDER_NAME[p] ?? p} — ${pr.hint || "Not signed in"}` : (PROVIDER_NAME[p] ?? p)}
                >
                  {#if hasMark(p)}
                    <ProviderLogo provider={p} size={18} />
                  {:else}
                    <span class="m-initial sm">{(p[0] ?? "?").toUpperCase()}</span>
                  {/if}
                </button>
              {/if}
            {/each}
          </div>
          <div class="model-list">
            {#if railSel !== "__fav"}
              {@const pr = models.find((r) => r.provider === railSel)}
              {@const st = pr ? statusOf(pr) : null}
              {#if pr && st && (st.kind === "off" || st.kind === "expired")}
                <div class="prov-head">
                  <button
                    class="signin"
                    on:click={() => { dispatch("command", { name: "keys", arg: pr.provider }); showModelPicker = false; }}
                  >{pr.hint || "Sign in"}</button>
                </div>
              {/if}
            {/if}
            {#each mainItems as row, i (row.value)}
              <button
                class="mrow"
                class:on={row.value === model || i === modelIndex}
                on:click={() => pickFamily(row)}
                on:mousemove={() => (modelIndex = i)}
              >
                {#if q && hasMark(row.provider)}
                  <ProviderLogo provider={row.provider} size={14} />
                {/if}
                <span class="meta">
                  <span class="nm">{row.label}</span>
                  <span class="sub">{planSub(row)}</span>
                </span>
                {#if i < 5}<kbd class="kbd">Ctrl+{i + 1}</kbd>{/if}
                {#if row.auth !== "ok" && row.provider !== "opencode"}
                  <span class="nokey">{row.auth === "expired" ? "expired" : "sign in"}</span>
                {/if}
                {#if row.provider !== "auto"}
                <span
                  class="star"
                  class:on={favorites.includes(row.value)}
                  role="button"
                  tabindex="0"
                  title="star model"
                  on:click|stopPropagation={() => toggleFav(row.value)}
                  on:keydown={(e) => e.key === "Enter" && toggleFav(row.value)}
                >{favorites.includes(row.value) ? "★" : "☆"}</span>
                {/if}
              </button>
            {/each}
            {#if mainItems.length === 0}
              <div class="empty">
                {refreshingProvider === railSel
                  ? "Fetching live catalog…"
                  : q
                    ? "No matching models found"
                    : "No models available — open Settings › Models"}
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if effortOpen}
      <div class="pop effort-pop from-left" class:from-top={effortBelow} style={effortPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        <div class="sec-h">Effort</div>
        {#each effortOpts as opt}
          <button class="opt-row" class:on={effort === opt.id} on:click={() => setEffort(opt.id)}>
            <span class="meta"><span class="nm">{opt.label}</span><span class="sub">{opt.hint}</span></span>
            {#if effort === opt.id}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
          </button>
        {/each}
        <div class="sec-h">Mode</div>
        {#each MODES as m}
          <button class="opt-row" class:on={mode === m.id} on:click={() => setMode(m.id)}>
            <span class="meta"><span class="nm">{m.title}</span><span class="sub">{m.desc}</span></span>
            {#if mode === m.id}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if wsOpen}
      <div class="pop ws-pop from-right" class:from-top={wsBelow} style={wsPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        <button class="opt-row" class:on={!workspace} on:click={() => pickWorkspace("")}>
          <span class="meta"><span class="nm">No workspace</span></span>
          {#if !workspace}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
        </button>
        {#each workspaces as w (w)}
          <button class="opt-row" class:on={workspace === w} on:click={() => pickWorkspace(w)}>
            <span class="p-ico"><Icon d={I.folder} size={13} /></span>
            <span class="meta"><span class="nm">{w}</span></span>
            {#if workspace === w}<span class="tick"><Icon d={I.check} size={12} /></span>{/if}
          </button>
        {/each}
        <button class="opt-row new-ws" on:click={() => { wsOpen = false; dispatch("newWorkspace"); }}>
          <span class="p-ico"><Icon d={I.plus} size={13} /></span>
          <span class="meta"><span class="nm">New workspace…</span></span>
        </button>
      </div>
    {/if}

    {#if permOpen}
      <div class="pop perm-pop from-right" class:from-top={permBelow} style={permPopStyle} transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}>
        {#each PERMS as p}
          <button class="opt-row perm" class:on={permission === p.id} on:click={() => setPermission(p.id)}>
            <span class="p-ico"><Icon d={p.icon} size={14} /></span>
            <span class="meta"><span class="nm">{p.title}</span><span class="sub">{p.desc}</span></span>
          </button>
        {/each}
      </div>
    {/if}
</div>

<style>
  /* Same surface recipe as the sidebar shell: near-black vertical
     gradient + 14px saturate blur. The card keeps its glass radius,
     shadow and top-edge highlight; only the paint now matches. */
  .ob { position: relative; width: 100%; max-width: 720px; margin: 0 auto; min-width: 0; }
  .ob-card {
    position: relative;
    background: linear-gradient(180deg, color-mix(in srgb, var(--parzi-sidebar) 88%, transparent), color-mix(in srgb, var(--parzi-sidebar) 78%, transparent));
    backdrop-filter: blur(14px) saturate(1.2);
    -webkit-backdrop-filter: blur(14px) saturate(1.2);
    border: 1px solid var(--line-2);
    border-top-color: var(--line-hi);
    border-radius: var(--glass-radius);
    box-shadow: var(--glass-shadow);
    padding: 14px 14px 10px;
    color: var(--text-2);
    min-width: 0;
    overflow: hidden;
    box-sizing: border-box;
  }
  .ob-card textarea {
    width: 100%; background: transparent; border: none; outline: none; resize: none;
    color: var(--text); font: inherit; font-size: 14px; line-height: 1.55;
    min-height: 26px; max-height: 180px; padding: 2px 4px 10px; box-sizing: border-box;
  }
  .ob-card textarea::placeholder { color: var(--text-4); }
  /* Borderless composer: kill the global focus ring (it draws a stray
     rectangle here) and answer with a subtle card-edge lift instead. */
  .ob-card textarea:focus { border-color: transparent !important; box-shadow: none !important; }
  .ob-card { transition: border-color 140ms ease; }
  .ob-card:focus-within { border-color: var(--line-3); }

  .attach-grid { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; overflow: hidden; }
  .thumb {
    position: relative; display: flex; align-items: center; gap: 7px;
    min-width: 0; max-width: 180px; height: 40px; padding: 0 12px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: 8px; color: var(--text-2); font-size: 12px; box-sizing: border-box;
  }
  .thumb.has-img { width: 76px; height: 58px; padding: 0; overflow: hidden; flex: none; }
  .thumb-img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .thumb-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--parzi-mono); font-size: 11px; }
  .thumb-name.over {
    position: absolute; left: 0; right: 0; bottom: 0; padding: 6px 6px 3px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.75));
    color: var(--text-2); font-size: 9px;
  }
  .thumb-x {
    position: absolute; top: -8px; right: -8px; width: 22px; height: 22px;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--menu); border: 1px solid var(--line);
    border-radius: 50%; color: var(--text-3); cursor: pointer; padding: 0;
  }
  .thumb-x:hover { color: var(--text); border-color: var(--bad); }
  .attach-clear {
    align-self: center; background: transparent; border: none; border-radius: 5px;
    color: var(--text-4); font: inherit; font-size: 11px; padding: 4px 8px; cursor: pointer;
  }
  .attach-clear:hover { color: var(--bad); background: var(--bad-soft); }
  .stage-err { font-size: 11px; color: var(--bad); padding: 2px 2px 6px; }

  .slash-pop, .at-popup {
    display: flex; flex-direction: column; gap: 1px; margin: 2px 0 8px;
    background: var(--menu); border: 1px solid var(--line);
    border-radius: 9px; padding: 5px; max-height: 220px; overflow-y: auto;
  }
  .slash-pop button, .at-popup button {
    display: flex; align-items: center; gap: 8px; background: transparent; border: none;
    border-radius: 6px; color: var(--text-2); text-align: left; padding: 6px 8px;
    cursor: pointer; font: inherit; font-size: 12.5px;
  }
  .slash-pop button.on, .slash-pop button:hover, .at-popup button.on, .at-popup button:hover {
    background: var(--surface-2); color: var(--text);
  }
  .slash-pop .n { flex: 1; font-family: var(--parzi-mono); font-size: 12px; }
  .slash-pop .h { margin-left: auto; font-size: 11px; color: var(--text-3); }
  .at-popup button { font-family: var(--parzi-mono); font-size: 12px; }

  .controls { display: flex; align-items: center; gap: 2px; min-width: 0; flex-wrap: wrap; row-gap: 4px; }
  .ctl-zone { position: static; min-width: 0; flex: 0 1 auto; display: flex; }
  .ctl {
    display: inline-flex; align-items: center; gap: 7px; max-width: 180px; min-width: 0; flex: 1 1 auto;
    background: transparent; border: none; border-radius: 7px; color: var(--text-3);
    font: inherit; font-size: 12.5px; padding: 6px 8px; cursor: pointer; text-align: left;
    overflow: hidden;
  }
  .ctl:hover { background: var(--surface-2); color: var(--text); }
  .ctl.open { background: var(--surface-3); color: var(--text); }
  .ctl .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ctl .chev { color: var(--text-3); display: inline-flex; flex: none; }
  .ctl:hover .chev, .ctl.open .chev { color: var(--text-2); }
  .ctl-glyph { display: inline-flex; color: var(--text-3); flex: none; }
  .ctl:hover .ctl-glyph, .ctl.open .ctl-glyph { color: var(--text); }
  .vdiv { width: 1px; height: 16px; background: var(--line-2); margin: 0 5px; flex: none; }
  .spacer { flex: 1 1 auto; min-width: 4px; }

  .ctx {
    height: 28px; flex: none; display: inline-flex; align-items: center; gap: 5px;
    padding: 0 8px; border: none; border-radius: var(--radius-pill); background: transparent;
    color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; cursor: pointer;
    --ctx: var(--text-3);
  }
  .ctx:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .ctx:disabled { cursor: default; }
  .ctx.warn { --ctx: var(--warn); color: var(--warn); }
  .ctx.bad { --ctx: var(--bad); color: var(--bad); }
  .ctx svg { transform: rotate(-90deg); }
  .ctx.busy svg { animation: ctx-spin 0.9s linear infinite; }
  .ctx-track { fill: none; stroke: var(--line-3); stroke-width: 2; }
  .ctx-fill { fill: none; stroke: var(--ctx); stroke-width: 2; stroke-linecap: round; transition: stroke-dashoffset 0.4s ease; }
  @keyframes ctx-spin { to { transform: rotate(270deg); } }

  .icon-btn {
    width: 32px; height: 32px; flex: none; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; border-radius: 50%; color: var(--text-3); cursor: pointer;
  }
  .icon-btn:hover { background: var(--surface-2); color: var(--text); }

  .go {
    width: 32px; height: 32px; flex: none; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-2); border: 1px solid var(--line-3);
    color: var(--text-2); cursor: pointer; padding: 0;
    box-shadow: none;
  }
  .go:hover:not(:disabled) { background: var(--surface-3); color: var(--text); }
  .go.ready:not(:disabled) { background: var(--accent); border-color: transparent; color: var(--accent-ink); }
  .go.ready:hover:not(:disabled) { background: var(--accent); color: var(--accent-ink); filter: brightness(1.08); }
  .go:active:not(:disabled) { transform: scale(0.94); }
  .go:disabled { opacity: 0.35; cursor: default; }
  .go.stop { background: var(--bad); border-color: transparent; color: var(--stage); }

  .pop {
    position: fixed; z-index: 60;
    background: var(--menu);
    border: 1px solid var(--line); border-radius: var(--glass-radius);
    box-shadow: var(--menu-shadow);
    overflow: hidden;
    transform-origin: bottom left;
  }
  .pop.from-right { transform-origin: bottom right; }
  .pop.from-top { transform-origin: top left; }
  .pop.from-top.from-right { transform-origin: top right; }
  .model-pop {
    width: 440px; max-width: calc(100vw - 16px);
    height: 430px; max-height: calc(100vh - 120px);
    display: flex; flex-direction: column;
  }
  .pick-body { display: flex; flex: 1; min-height: 0; }
  .prov-rail {
    flex: none; width: 54px; display: flex; flex-direction: column; gap: 2px;
    padding: 6px 5px; overflow-y: auto; border-right: 1px solid var(--line-2);
  }
  .rail-row {
    display: flex; align-items: center; justify-content: center; width: 100%; height: 36px; flex: none;
    background: transparent; border: none; border-radius: 8px; color: var(--text-2);
    padding: 0; cursor: pointer;
  }
  .rail-row:hover { background: var(--surface-2); color: var(--text); }
  .rail-row.on { background: var(--surface-3); color: var(--text); }
  .rail-row.dim { opacity: 0.4; }
  .rail-row.dim:hover { opacity: 0.8; }
  .rail-auto { display: inline-flex; color: var(--accent-text); }
  .rail-row.on .rail-auto { color: var(--accent-text); }
  .rail-star { flex: none; color: var(--warn); font-size: 15px; line-height: 1; }
  .model-pop .model-list { border: none; padding: 5px 5px 5px 4px; max-height: none; }
  .m-initial.sm { width: 20px; height: 20px; font-size: 10px; border-radius: 6px; }
  .effort-pop { width: 300px; max-width: calc(100vw - 16px); padding: 5px; overflow-y: auto; }
  .ws-pop { width: 240px; max-width: calc(100vw - 16px); padding: 5px; overflow-y: auto; }
  .ws-pop .new-ws { color: var(--text-3); border-top: 1px solid var(--line-2); border-radius: 0 0 7px 7px; margin-top: 3px; }
  .ctl.locked { cursor: default; }
  .perm-pop { width: 320px; max-width: calc(100vw - 16px); padding: 5px; overflow-y: auto; }

  .pop-search {
    display: flex; align-items: center; gap: 8px; padding: 8px 10px; flex: none;
    color: var(--text-3); border-bottom: 1px solid var(--line-2);
  }
  .pop-search input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 13px; box-shadow: none !important;
  }
  .pop-search input::placeholder { color: var(--text-4); }

  .model-list { flex: 1; min-width: 0; min-height: 0; max-height: 380px; overflow-y: auto; padding: 5px; display: flex; flex-direction: column; gap: 1px; }
  .spin {
    flex: none; width: 11px; height: 11px; border-radius: 50%;
    border: 1.6px solid var(--line-3); border-top-color: var(--text-2);
    animation: obspin 0.9s linear infinite;
  }
  @keyframes obspin { to { transform: rotate(360deg); } }
  .prov-head {
    display: flex; align-items: center; gap: 8px; width: 100%;
    padding: 6px 8px 4px; color: var(--text-2);
  }

  .signin {
    margin-left: auto; flex: none; font: inherit; font-size: 11.5px; font-weight: 600;
    color: var(--accent-text); background: var(--accent-soft);
    border: 1px solid var(--accent-line); border-radius: 7px;
    padding: 4px 10px; cursor: pointer; max-width: 180px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .signin:hover { filter: brightness(1.06); }
  .sec-h { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.07em; color: var(--text-3); padding: 8px 8px 4px; }
  .mrow, .opt-row {
    display: flex; align-items: center; gap: 9px; width: 100%;
    background: transparent; border: none; border-radius: 7px; color: var(--text-2);
    text-align: left; padding: 6px 8px; cursor: pointer; font: inherit; font-size: 13px;
  }
  .mrow:hover, .mrow.on, .opt-row:hover, .opt-row.on { background: var(--surface-2); color: var(--text); }
  .opt-row.on { background: var(--surface-3); color: var(--text); }
  .m-initial {
    width: 22px; height: 22px; flex: none; display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-2); border: 1px solid var(--line-2);
    border-radius: 6px; color: var(--text-2); font-size: 11px; font-weight: 600;
  }
  .meta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .opt-row.perm .nm { font-weight: 600; font-size: 12.5px; }
  .sub { font-size: 11px; color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; line-height: 1.4; }
  .opt-row.perm .sub { white-space: normal; }
  .kbd {
    font-family: var(--parzi-mono); font-size: 10px; color: var(--text-3); flex: none;
    border: 1px solid var(--line-3); border-radius: 4px; padding: 1px 5px;
    opacity: 0; transition: opacity 120ms ease;
  }
  .mrow:hover .kbd, .mrow.on .kbd { opacity: 1; }
  .nokey { font-size: 11px; color: var(--bad); flex: none; }
  .star { color: var(--text-4); font-size: 13px; padding: 1px 4px; cursor: pointer; flex: none; opacity: 0; transition: opacity 120ms ease; }
  .mrow:hover .star, .mrow:focus-within .star, .star.on { opacity: 1; }
  .star:hover { color: var(--text); }
  .star.on { color: var(--warn); }
  .tick { color: var(--text-2); display: inline-flex; flex: none; }
  .p-ico { display: inline-flex; color: var(--text-2); flex: none; }
  .empty { padding: 14px; text-align: center; color: var(--text-3); font-size: 12px; }

  @media (prefers-reduced-motion: reduce) {
    .go:active:not(:disabled) { transform: none; }
    .kbd, .star { transition: none; }
  }
  @media (max-width: 560px) {
    .ob-card { padding: 12px 10px 8px; }
    .ctl { max-width: 128px; font-size: 12px; padding: 6px 6px; gap: 5px; }
    .vdiv { margin: 0 2px; }
  }
</style>
