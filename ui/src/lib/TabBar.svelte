<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import type { Tab } from "./tabs";
  import { faviconUrl } from "./browserData";
  import { pressDrag, tabDragStarted, tabRelease, type PressDrag } from "./gestures";
  import { sessionMarks } from "./activity";
  import { tooltip } from "./tooltip";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let vertical = false;
  export let threads: { id: string; model: string }[] = [];

  // The agent behind a session tab, from the thread's model.
  const providerOf = (sid?: string | null) => {
    const m = sid ? threads.find((t) => t.id === sid)?.model : undefined;
    return m ? m.split("/")[0] : "";
  };
  // A draft tab has no thread yet: the logo of the model it will start
  // with, from its own composer (seeded from the last send).
  const draftProvider = (tab: Tab) => {
    const m = tab.composer?.model;
    return m ? m.split("/")[0] : "";
  };

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
  }>();

  let strip: HTMLElement;
  const els: Record<string, HTMLElement> = {};
  let broken = new Set<string>();
  let dragId = "";
  let dragX = 0;
  let order: string[] | null = null;
  // Set when a drag swallowed the click, so that click does not select again.
  let swallow = "";

  $: shown = order ? order.map((id) => tabs.find((t) => t.id === id)).filter((t): t is Tab => !!t) : tabs;
  $: markOf = (sessionId?: string | null) => (sessionId ? $sessionMarks[sessionId] : undefined);
  $: markTitle = (title: string, sessionId?: string | null) =>
    markOf(sessionId) === "waiting" ? `${title} · waiting for you` : markOf(sessionId) === "running" ? `${title} · working` : title;

  // The tab you are on stays in view when the strip overflows.
  $: if (strip && activeTabId) void tick().then(() => strip?.querySelector('[aria-selected="true"]')?.closest(".tab")?.scrollIntoView({ block: "nearest", inline: "nearest" }));

  let collapsed = new Set<string>();
  let touched = new Set<string>();

  // Groups start collapsed (agent tabs arrive quietly); explicit
  // user toggles win from then on. Guarded: only reassign when the set
  // actually changed, otherwise `collapsed = collapsed` self-invalidates
  // (objects are always dirty) and loops the reactive flush forever,
  // freezing the UI on the next tabs change / keystroke.
  $: {
    let changed = false;
    for (const [sid, kids] of groups.kids) {
      if (kids.length && !touched.has(sid) && !collapsed.has(sid)) {
        collapsed.add(sid);
        changed = true;
      }
    }
    if (changed) collapsed = new Set(collapsed);
  }

  // Pages opened from a session (by its agent, or a link in its thread)
  // attach under its tab instead of cluttering the strip: one framed
  // group, the session first, its pages after it.
  $: groups = (() => {
    const bySession = new Map<string, Tab>();
    for (const t of shown) if (t.kind === "session" && t.sessionId) bySession.set(t.sessionId, t);
    const kids = new Map<string, Tab[]>();
    const lone: Tab[] = [];
    for (const t of shown) {
      const parent = t.kind === "page" ? t.owner || t.group : t.kind === "session" || t.kind === "artifact" ? t.group : null;
      if (parent && bySession.has(parent)) {
        const arr = kids.get(parent) ?? [];
        arr.push(t);
        kids.set(parent, arr);
      } else {
        lone.push(t);
      }
    }
    return { bySession, kids, lone };
  })();

  const hostOfUrl = (url?: string) => {
    try {
      return url ? new URL(url).hostname.replace(/^www\./, "") : "page";
    } catch {
      return "page";
    }
  };

  function toggleGroup(sessionId: string) {
    touched = new Set(touched).add(sessionId);
    collapsed = new Set(collapsed);
    if (collapsed.has(sessionId)) collapsed.delete(sessionId);
    else collapsed.add(sessionId);
  }

  async function onKey(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      dispatch("select", { id });
    } else if (e.ctrlKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      const at = tabs.findIndex((t) => t.id === id);
      const back = e.key === "ArrowLeft" || e.key === "ArrowUp";
      dispatch("move", { id, to: Math.max(0, at + (back ? -1 : 1)) });
      await tick();
      els[id]?.querySelector<HTMLElement>('[role="tab"]')?.focus();
    }
  }

  function onAux(e: MouseEvent, id: string) {
    if (e.button === 1) dispatch("close", { id });
  }

  function onPick(id: string) {
    if (swallow === id) {
      swallow = "";
      return;
    }
    dispatch("select", { id });
  }

  type Press = { id: string; x: number; y: number; grab: number };

  // A press on a tab: a click selects through the tab's own handler, a drag
  // past the slop moves it.
  const drag = (id: string): PressDrag<Press> => ({
    press(e) {
      swallow = "";
      if ((e.target as HTMLElement).closest(".close")) return null;
      const el = els[id];
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { id, x: e.clientX, y: e.clientY, grab: vertical ? e.clientY - r.top : e.clientX - r.left };
    },
    starts: (p, e) => tabDragStarted(p.x, p.y, e.clientX, e.clientY, vertical),
    start(p) {
      dragId = p.id;
      order = tabs.map((t) => t.id);
    },
    drag: (p, e) => void follow(p, vertical ? e.clientY : e.clientX),
    release(p) {
      const result = tabRelease({ pressId: p.id, dragId, order, ids: tabs.map((t) => t.id) });
      reset();
      if (result.kind === "move") {
        swallow = result.id;
        dispatch("move", { id: result.id, to: result.to });
        dispatch("select", { id: result.id });
      } else if (result.kind === "select") {
        swallow = result.id;
        dispatch("select", { id: result.id });
      }
    },
    cancel() {
      reset();
      swallow = "";
    },
  });

  async function follow(press: Press, client: number) {
    const el = els[dragId];
    if (!el || !order) return;
    const box = strip.getBoundingClientRect();
    const size = (id: string) => (els[id] ? (vertical ? els[id].offsetHeight : els[id].offsetWidth) : 0);
    const start = (id: string) => (els[id] ? (vertical ? els[id].offsetTop : els[id].offsetLeft) : 0);
    const end = Math.max(...order.map((id) => (els[id] ? start(id) + size(id) : 0)));
    const scroll = vertical ? strip.scrollTop : strip.scrollLeft;
    const origin = vertical ? box.top : box.left;
    const pos = Math.max(0, Math.min(client - origin + scroll - press.grab, end - size(dragId)));
    const far = pos + size(dragId);
    const mine = order.indexOf(dragId);
    const before = order.filter((id, i) => {
      if (id === dragId || !els[id]) return false;
      const center = start(id) + size(id) / 2;
      return i < mine ? pos >= center : far > center;
    });
    const next = [...before, dragId, ...order.filter((id) => id !== dragId && !before.includes(id))];
    if (next.join() !== order.join()) {
      order = next;
      await tick();
    }
    if (!dragId) return;
    dragX = pos - start(dragId);
  }

  function reset() {
    dragId = "";
    dragX = 0;
    order = null;
  }
</script>

<!-- A tablist may only own tabs, so each tab is a plain pill holding the
     role="tab" element and, beside it, pointer-only caret and close
     buttons (hidden from assistive tech; Ctrl+W closes the active tab). -->
<div class="tabs" class:vertical class:none={!shown.length} role="tablist" bind:this={strip}>
  {#each groups.lone as tab (tab.id)}
    {@const attached = tab.kind === "session" && tab.sessionId ? (groups.kids.get(tab.sessionId) ?? []) : []}
    {@const shut = tab.sessionId ? collapsed.has(tab.sessionId) && !attached.some((a) => a.id === activeTabId) : false}
    {@const linked = `${attached.length} page${attached.length === 1 ? "" : "s"} from this session`}
    {@const provider = tab.kind === "session" ? providerOf(tab.sessionId) || draftProvider(tab) : ""}
    {#if attached.length && !shut}
      <div class="group">
        <div
          bind:this={els[tab.id]}
          class="tab"
          class:active={tab.id === activeTabId}
          class:dragging={tab.id === dragId}
          style={tab.id === dragId ? `transform: ${vertical ? "translateY" : "translateX"}(${dragX}px)` : ""}
          use:pressDrag={drag(tab.id)}
          on:auxclick={(e) => onAux(e, tab.id)}
        >
          <div
            class="pick"
            role="tab"
            tabindex="0"
            aria-selected={tab.id === activeTabId}
            title={markTitle(tab.title, tab.sessionId)}
            on:click={() => onPick(tab.id)}
            on:keydown={(e) => onKey(e, tab.id)}
          >
            <span class="kind">
              {#if tab.loading || markOf(tab.sessionId) === "running"}
                <span class="spinner" />
              {:else if tab.sessionId && hasMark(providerOf(tab.sessionId))}
                <ProviderLogo provider={providerOf(tab.sessionId)} size={14} />
              {:else}
                <Icon name="bot" />
              {/if}
            </span>
            <span class="title">{tab.title || "New session"}</span>
            {#if markOf(tab.sessionId) === "waiting"}<span class="waiting" />{/if}
          </div>
          <button
            class="close count on"
            tabindex="-1"
            aria-hidden="true"
            use:tooltip={`Hide ${linked}`}
            on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
          >
            {attached.length}
          </button>
          <button
            class="close"
            tabindex="-1"
            aria-hidden="true"
            use:tooltip={"Close tab"}
            on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
          >
            <Icon name="close" size={12} />
          </button>
        </div>
        {#each attached as sub (sub.id)}
          <div class="tab sub" class:active={sub.id === activeTabId} on:auxclick={(e) => onAux(e, sub.id)}>
            <div
              class="pick"
              role="tab"
              tabindex="0"
              aria-selected={sub.id === activeTabId}
              title={`${tab.title || "Session"} › ${sub.title || sub.url || "New page"}`}
              on:click={() => onPick(sub.id)}
              on:keydown={(e) => onKey(e, sub.id)}
            >
            <span class="kind">
              {#if sub.loading || markOf(sub.sessionId) === "running"}
                <span class="spinner" />
              {:else if sub.kind === "session" && sub.sessionId && hasMark(providerOf(sub.sessionId))}
                <ProviderLogo provider={providerOf(sub.sessionId)} size={14} />
              {:else if sub.kind === "session"}
                <Icon name="bot" />
              {:else if sub.kind === "artifact"}
                  <Icon name="file" />
                {:else if sub.url && !broken.has(sub.url)}
                  <img src={sub.favicon || faviconUrl(sub.url)} alt="" on:error={() => sub.url && (broken = new Set(broken).add(sub.url))} />
                {:else}
                  <Icon name="globe" />
                {/if}
              </span>
              <span class="title">{sub.title || (sub.kind === "session" ? "Side task" : "New page")}</span>
              {#if markOf(sub.sessionId) === "waiting"}<span class="waiting" />{/if}
            </div>
            <button
              class="close"
              tabindex="-1"
              aria-hidden="true"
              use:tooltip={"Close tab"}
              on:click|stopPropagation={() => dispatch("close", { id: sub.id })}
            >
              <Icon name="close" size={12} />
            </button>
          </div>
        {/each}
      </div>
    {:else}
    <div
      bind:this={els[tab.id]}
      class="tab"
      class:active={tab.id === activeTabId}
      class:dragging={tab.id === dragId}
      style={tab.id === dragId ? `transform: ${vertical ? "translateY" : "translateX"}(${dragX}px)` : ""}
      use:pressDrag={drag(tab.id)}
      on:auxclick={(e) => onAux(e, tab.id)}
    >
      <div
        class="pick"
        role="tab"
        tabindex="0"
        aria-selected={tab.id === activeTabId}
        title={tab.kind === "page" ? tab.url || tab.title : markTitle(tab.title, tab.sessionId)}
        on:click={() => onPick(tab.id)}
        on:keydown={(e) => onKey(e, tab.id)}
      >
            <span class="kind">
              {#if tab.loading || markOf(tab.sessionId) === "running"}
                <span class="spinner" />
              {:else if tab.kind === "page" && tab.url && !broken.has(tab.url)}
                <img src={tab.favicon || faviconUrl(tab.url)} alt="" on:error={() => tab.url && (broken = new Set(broken).add(tab.url))} />
              {:else if provider && hasMark(provider)}
                <ProviderLogo {provider} size={14} />
              {:else if tab.kind === "session" && !tab.sessionId}
                <Icon name="home" />
              {:else}
                <Icon name={tab.kind === "page" ? "globe" : tab.kind === "brain" ? "brain" : tab.kind === "history" ? "clock" : tab.kind === "settings" ? "settings" : tab.kind === "preview" ? "spark" : tab.kind === "artifact" ? "file" : tab.sessionId?.startsWith("r:") || tab.sessionId?.startsWith("m:") ? "server" : "bot"} />
              {/if}
            </span>
        <span class="title">{tab.title || (tab.kind === "page" ? "New page" : "New session")}</span>
        {#if markOf(tab.sessionId) === "waiting"}<span class="waiting" />{/if}
      </div>
      {#if attached.length}
        <button
          class="close count"
          tabindex="-1"
          aria-hidden="true"
          use:tooltip={`Show ${linked}: ${attached.map((a) => a.title || hostOfUrl(a.url)).join(", ")}`}
          on:click|stopPropagation={() => tab.sessionId && toggleGroup(tab.sessionId)}
        >
          {attached.length}
        </button>
      {/if}
      <button
        class="close"
        tabindex="-1"
        aria-hidden="true"
        use:tooltip={"Close tab"}
        on:click|stopPropagation={() => dispatch("close", { id: tab.id })}
      >
        <Icon name="close" size={12} />
      </button>
    </div>
    {/if}
  {/each}
</div>
{#if !vertical}
  <button class="new" use:tooltip={"New session (Ctrl+T)"} on:click={() => dispatch("newTab")}>
    <Icon name="plus" />
  </button>
{/if}

<style>
  .tabs {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow-x: auto;
  }
  /* No tabs: no slot, so Home and + sit one gap apart. */
  .tabs.none {
    display: none;
  }
  .tabs.vertical {
    flex-direction: column;
    align-items: stretch;
    overflow-x: hidden;
    overflow-y: auto;
  }
  .tabs.vertical .tab {
    max-width: none;
    width: 100%;
    flex: none;
  }
  .tabs.vertical .group {
    width: 100%;
    flex-direction: column;
    align-items: stretch;
  }
  .tabs.vertical .tab.sub {
    max-width: none;
  }
  /* Each tab fits its title, up to a limit; the fill is the only state. */
  .tab {
    display: inline-flex;
    align-items: center;
    height: 28px;
    max-width: 220px;
    flex: 0 1 auto;
    min-width: 96px;
    padding: 0 4px 0 0;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
    transition: background 140ms ease, color 140ms ease, transform 160ms ease;
  }
  .tab.dragging {
    position: relative;
    z-index: 2;
    background: var(--panel);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
    cursor: grabbing;
    transition: background 140ms ease, color 140ms ease;
  }
  .tab:hover {
    background: var(--hover);
    color: var(--text);
  }
  /* Glass tinted with the theme's panel, not a white veil: the title stays
     readable when a light theme's dark text sits over a dimmed picture. */
  .tab.active {
    background: color-mix(in srgb, var(--panel) 82%, transparent);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    color: var(--text);
  }
  /* The tab proper: fills the pill up to its buttons. Focus rings the pill. */
  .pick {
    flex: 1;
    align-self: stretch;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 10px;
    border-radius: inherit;
  }
  .pick:focus-visible {
    outline: none;
  }
  .tab:has(> .pick:focus-visible) {
    outline: 2px solid color-mix(in srgb, var(--accent) 70%, transparent);
    outline-offset: 2px;
  }
  .kind {
    display: inline-flex;
    flex: none;
    color: var(--muted);
  }
  .kind img {
    width: 14px;
    height: 14px;
  }
  .waiting {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--warn);
  }
  .spinner {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.6px solid var(--line);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tab.active .kind {
    color: var(--accent);
  }
  /* A page under its session: same type as any tab, a little narrower,
     inside the session's frame. */
  .tab.sub {
    height: 24px;
    max-width: 168px;
    min-width: 80px;
  }
  /* The session and its pages read as one unit: framed and tinted. */
  .group {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--text) 5%, transparent);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  /* How many pages and side tasks sit under a session; a click shows or
     hides them. */
  .close.count {
    opacity: 1;
    width: auto;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--hover);
    background-clip: border-box;
    font-size: var(--fs-xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .close.count.on {
    background: var(--selected);
    color: var(--text);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* 24px hit area; the padding is clipped so the hover chip stays 16px. */
  .close {
    width: 24px;
    height: 24px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    background: transparent;
    background-clip: content-box;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .close + .close {
    margin-left: 4px;
  }
  .tab:hover .close,
  .tab.active .close {
    opacity: 1;
  }
  .close:hover {
    background: var(--selected);
    color: var(--text);
  }
  .new {
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .new:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
