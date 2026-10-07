<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, type ChatEvent, type SessionMeta } from "./api";
  import { folderName } from "./tabs";
  import { renderMarkdown } from "./md";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let folder = "";
  export let sessionId = "";
  // Dock tabs are a registry: append { id, label } plus a content branch
  // below to add projects, checklists, etc. without touching the shell.
  type DockTab = "ask" | "agents";
  const DOCK_TABS: { id: DockTab; label: string }[] = [
    { id: "ask", label: "Ask" },
    { id: "agents", label: "Agents" },
  ];

  export let tab: DockTab = "ask";

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    stopSession: { id: string };
    close: void;
    settled: void;
  }>();

  interface AskCard {
    key: number;
    q: string;
    sid: string;
    answer: string;
    busy: boolean;
  }

  let askInput = "";
  let asking = false;
  let cards: AskCard[] = [];
  let seq = 0;
  let field: HTMLTextAreaElement | null = null;
  let poller = 0;
  let w = 360;
  try {
    const saved = Number(localStorage.getItem("parzi.dock.w"));
    if (saved >= 280 && saved <= 640) w = saved;
  } catch {}

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = w;
    const move = (ev: PointerEvent) => {
      w = Math.min(640, Math.max(280, Math.round(startW + (startX - ev.clientX))));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      try {
        localStorage.setItem("parzi.dock.w", String(w));
      } catch {}
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function agentOf(s: SessionMeta) {
    return s.model.split("/")[0];
  }

  function laneFallback(s: SessionMeta): "brain" | "bot" | "chat" {
    const l = s.lane ?? "";
    const lane = l === "code" ? "build" : l;
    return lane === "research" ? "brain" : lane === "build" ? "bot" : "chat";
  }

  function isLive(s: SessionMeta) {
    return running.has(s.id) || s.status === "active";
  }

  function pillOf(s: SessionMeta): { label: string; cls: string } | null {
    if (isLive(s)) return { label: "Running", cls: "live" };
    if (s.status === "queued") return { label: "Queued", cls: "queue" };
    return null;
  }

  function whereOf(s: SessionMeta) {
    if (!s.cwd || /[\\/]\.parzi[\\/]scratch[\\/]/.test(s.cwd)) return "";
    return folderName(s.cwd);
  }

  function time(iso: string) {
    const at = Date.parse(iso);
    if (!at) return "";
    return new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }

  $: q = askInput.trim().toLowerCase();
  $: family = (() => {
    if (!sessionId) return [];
    const byId = new Map(threads.map((t) => [t.id, t]));
    const out: SessionMeta[] = [];
    for (const t of threads) {
      let cur = t.parent_id;
      let guard = 0;
      while (cur && guard++ < 50) {
        if (cur === sessionId) {
          out.push(t);
          break;
        }
        cur = byId.get(cur)?.parent_id;
      }
    }
    return out;
  })();
  $: pool = family
    .filter((t) => !q || t.title.toLowerCase().includes(q) || t.model.toLowerCase().includes(q))
    .map((t) => ({ ...t, at: Date.parse(t.updated) || 0 }));
  $: activeList = pool.filter((t) => isLive(t) || t.status === "queued").sort((a, b) => b.at - a.at);
  $: rows = (() => {
    const kids = new Map<string, typeof pool>();
    for (const t of pool) {
      const p = t.parent_id ?? "";
      if (!kids.has(p)) kids.set(p, []);
      kids.get(p)?.push(t);
    }
    const byId = new Map(pool.map((t) => [t.id, t]));
    const roots = (kids.get(sessionId) ?? []).concat(pool.filter((t) => t.parent_id && t.parent_id !== sessionId && !byId.has(t.parent_id)));
    const byTime = (a: { at: number }, b: { at: number }) => b.at - a.at;
    roots.sort(byTime);
    for (const arr of kids.values()) arr.sort(byTime);
    const out: { s: (typeof pool)[number]; depth: number }[] = [];
    const walk = (id: string, depth: number) => {
      for (const c of kids.get(id) ?? []) {
        out.push({ s: c, depth });
        walk(c.id, depth + 1);
      }
    };
    for (const r of roots) {
      out.push({ s: r, depth: 0 });
      walk(r.id, 1);
    }
    return out;
  })();

  function answerOf(events: ChatEvent[]): string {
    let out = "";
    for (const e of events) if (e.kind === "assistant") out = e.text;
    return out;
  }

  async function poll(card: AskCard) {
    try {
      const [meta, events] = await api.getThread(card.sid);
      card.answer = answerOf(events);
      cards = cards;
      if (meta.status === "active" || meta.status === "queued" || running.has(card.sid)) return;
    } catch {
      return;
    }
    card.busy = false;
    cards = cards;
    dispatch("settled");
  }

  function poke() {
    clearInterval(poller);
    if (cards.some((c) => c.busy)) {
      poller = window.setInterval(() => {
        for (const c of cards) if (c.busy) void poll(c);
        if (!cards.some((c) => c.busy)) clearInterval(poller);
      }, 1500);
    }
  }

  $: cards, poke();

  onDestroy(() => clearInterval(poller));

  async function sendAsk() {
    const prompt = askInput.trim();
    if (!prompt || asking) return;
    asking = true;
    const input = prompt;
    askInput = "";
    try {
      const sid = await api.sendMessage({
        sessionId: null,
        model: "auto",
        prompt: input,
        cwd: folder,
        effort: "low",
        attachments: [],
        mode: "auto",
        lane: "research",
      });
      const card: AskCard = { key: ++seq, q: input, sid, answer: "", busy: true };
      cards = [card, ...cards];
      void poll(card);
    } catch (e) {
      askInput = input;
    } finally {
      asking = false;
    }
  }

  async function stopAsk(card: AskCard) {
    try {
      await api.killRun(card.sid);
    } catch {}
    card.busy = false;
    cards = cards;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void sendAsk();
    }
  }
</script>

<div class="panel" style:width="{w}px">
  <div class="grip" on:pointerdown={startResize} title="Drag to resize the dock" />
  <div class="p-head">
    <div class="p-tabs" role="tablist" aria-label="Dock">
      {#each DOCK_TABS as t (t.id)}
        <button
          role="tab"
          aria-selected={tab === t.id}
          class:on={tab === t.id}
          on:click={() => (tab = t.id)}
        >
          {t.label}{#if t.id === "agents" && activeList.length}<span class="n">{activeList.length}</span>{/if}
        </button>
      {/each}
    </div>
    <button class="x" title="Close panel" on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
  </div>

  {#if tab === "ask"}
    <div class="ask-box">
      <textarea
        bind:this={field}
        bind:value={askInput}
        rows="2"
        placeholder="Quick question — research lane, never asks"
        on:keydown={onKey}
      />
      <button class="go" disabled={!askInput.trim() || asking} title="Send (Enter)" on:click={sendAsk}>
        <Icon name="enter" size={14} />
      </button>
    </div>
    <div class="cards">
      {#each cards as c (c.key)}
        <div class="card">
          <div class="q">{c.q}</div>
          <div class="a">
            {#if c.answer}{@html renderMarkdown(c.answer)}{:else}<span class="thinking">Thinking…</span>{/if}
          </div>
          {#if c.busy}
            <button class="stop" on:click={() => stopAsk(c)}>Stop</button>
          {/if}
        </div>
      {:else}
        <p class="empty">Ask anything while your code runs. Answers are research sessions — find them in history later.</p>
      {/each}
    </div>
  {:else if tab === "agents"}
    <div class="agents">
      {#if activeList.length}
        <h3>Active now</h3>
        {#each activeList as s (s.id)}
          {@const pill = pillOf(s)}
          <div class="row">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if pill}<span class="pill {pill.cls}">{pill.label}</span>{/if}
            </button>
            <button class="stop" title="Stop this run (keeps the transcript)" on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
          </div>
        {/each}
      {/if}
      {#if rows.length}
        <h3>This session</h3>
        {#each rows as { s, depth } (s.id)}
          {@const sub = pillOf(s)}
          <div class="row" style:padding-left="{depth * 16}px">
            <button class="open" title={s.title} on:click={() => dispatch("openSession", { id: s.id })}>
              <span class="time">{time(s.updated)}</span>
              <span class="icon">
                {#if hasMark(agentOf(s))}<ProviderLogo provider={agentOf(s)} size={13} />{:else}<Icon name={laneFallback(s)} size={13} />{/if}
              </span>
              <span class="title">{s.title || "Untitled session"}</span>
              {#if sub}<span class="pill {sub.cls}">{sub.label}</span>{:else}<span class="url">{whereOf(s)}</span>{/if}
            </button>
            {#if sub}
              <button class="stop" title="Stop this run (keeps the transcript)" on:click={() => dispatch("stopSession", { id: s.id })}>Stop</button>
            {/if}
          </div>
        {/each}
      {:else}
        <p class="empty">{q ? "Nothing matches." : "No subagents yet — this session hasn't fanned anything out."}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .panel {
    position: relative;
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
    background: var(--glass-bg);
    -webkit-backdrop-filter: var(--glass-strong-blur);
    backdrop-filter: var(--glass-strong-blur);
  }
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    width: 9px;
    cursor: ew-resize;
    z-index: 5;
  }
  .grip:hover::after,
  .grip:active::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
    opacity: 0.7;
  }
  .p-head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 10px 6px;
  }
  .p-tabs {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--text) 5%, transparent);
  }
  .p-tabs button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    background: none;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: var(--faint);
    font-size: 12px;
    cursor: pointer;
  }
  .p-tabs button.on {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    color: var(--text);
  }
  .n {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--ok);
    color: var(--on-ok);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
  }
  .x {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
  }
  .x:hover { background: var(--line); color: var(--text); }
  h3 {
    margin: 12px 12px 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .empty { margin: 24px 16px; font-size: 12.5px; color: var(--faint); line-height: 1.6; }
  .ask-box {
    flex: none;
    display: flex;
    align-items: flex-end;
    gap: 6px;
    margin: 2px 10px 0;
    padding: 8px 8px 8px 12px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .ask-box textarea {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    resize: none;
    color: var(--text);
    font-size: 13px;
    line-height: 1.45;
  }
  .go {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
  }
  .go:not(:disabled) { color: var(--text); background: color-mix(in srgb, var(--text) 12%, transparent); }
  .go:disabled { cursor: default; }
  .cards, .agents { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: 16px; }
  .card {
    margin: 10px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    font-size: 13px;
  }
  .q { color: var(--muted); font-size: 12px; margin-bottom: 6px; }
  .a { color: var(--text); line-height: 1.6; overflow-wrap: break-word; }
  .thinking { color: var(--faint); }
  .row { display: flex; align-items: center; gap: 2px; padding: 2px 8px 2px 4px; }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    text-align: left;
    cursor: pointer;
  }
  .open:hover { background: var(--line); color: var(--text); }
  .time { flex: none; font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .icon { flex: none; display: inline-flex; color: var(--muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
  .url { flex: none; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; color: var(--faint); }
  .pill { flex: none; padding: 1px 8px; border-radius: 999px; font-size: 10.5px; font-weight: 600; }
  .pill.live { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  .pill.queue { background: color-mix(in srgb, var(--warn) 16%, transparent); color: var(--warn); }
  .stop {
    flex: none;
    padding: 4px 10px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 11.5px;
    cursor: pointer;
  }
  .stop:hover { border-color: var(--bad); color: var(--bad); }
</style>
