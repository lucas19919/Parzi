<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import Thread from "./Thread.svelte";
  import Icon from "./Icon.svelte";
  import type { ChatEvent, ProviderStatus, SessionMeta } from "./api";
  import { remote, type RemoteApproval, type RemoteInfo } from "./remote";
  import { PROVIDER_ORDER, modelLabel, nameOf } from "./providerRows";
  import { chime, toast, toastError } from "./toast";

  export let visible = true;

  const dispatch = createEventDispatcher<{ setup: void }>();
  const FAST = 2_000;
  const SLOW = 8_000;
  const HIDDEN = 30_000;

  let info: RemoteInfo | null = null;
  let loaded = false;
  let reachable: boolean | null = null;
  let reason = "";
  let sessions: SessionMeta[] = [];
  let selected: string | null = null;
  let events: ChatEvent[] = [];
  let have = 0;
  let approvals: RemoteApproval[] = [];
  let agents: ProviderStatus[] = [];
  let showAgents = false;
  let checkingAgents = false;
  let model = "";
  let input = "";
  let sending = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let alive = true;
  let seen = new Set<string>();

  $: ready = agents.filter((a) => a.state === "ready");
  $: choices = ready.flatMap((a) => [
    { value: a.provider, label: `${nameOf(a.provider)} (default)` },
    ...a.models.map((m) => ({ value: `${a.provider}/${m.id}`, label: `${nameOf(a.provider)} · ${modelLabel(m)}` })),
  ]);
  $: if (!choices.some((c) => c.value === model)) model = choices[0]?.value ?? "";
  $: current = sessions.find((s) => s.id === selected) ?? null;
  $: busy = current?.status === "active" || current?.status === "queued";
  $: waiting = approvals.filter((a) => a.session === selected);

  onMount(() => {
    void start();
  });

  onDestroy(() => {
    alive = false;
    if (timer) clearTimeout(timer);
  });

  async function start() {
    try {
      info = await remote.info();
    } catch (e) {
      toastError(e);
    }
    loaded = true;
    if (!info) return;
    await poll();
    await checkAgents(false);
  }

  function reschedule(ms?: number) {
    if (!alive || !info) return;
    if (timer) clearTimeout(timer);
    const wait = ms ?? (!visible ? HIDDEN : busy || approvals.length > 0 ? FAST : SLOW);
    timer = setTimeout(() => void poll(), wait);
  }

  async function poll() {
    if (timer) clearTimeout(timer);
    timer = null;
    try {
      const [list, asks] = await Promise.all([remote.sessions(), remote.approvals()]);
      sessions = list;
      approvals = asks;
      reachable = true;
      reason = "";
      const fresh = asks.filter((a) => !seen.has(a.key));
      if (fresh.length) {
        seen = new Set(asks.map((a) => a.key));
        chime(true);
        if (!visible) toast(`${info?.label ?? "The remote"} needs an approval`);
      }
      if (selected) await loadEvents();
    } catch (e) {
      reachable = false;
      reason = String(e);
    }
    reschedule();
  }

  async function loadEvents() {
    const id = selected;
    if (!id) return;
    const r = await remote.events(id, have);
    if (selected !== id) return;
    events = r.from === have ? [...events, ...r.events] : r.events;
    have = r.total;
  }

  async function checkAgents(refresh: boolean) {
    checkingAgents = true;
    try {
      const all = await remote.providers(refresh);
      agents = PROVIDER_ORDER.map((id) => all.find((a) => a.provider === id)).filter((a): a is ProviderStatus => !!a);
      if (!agents.some((a) => a.state === "ready")) showAgents = true;
    } catch (e) {
      if (refresh) toastError(e);
    } finally {
      checkingAgents = false;
    }
  }

  function pick(id: string | null) {
    if (id === selected) return;
    selected = id;
    events = [];
    have = 0;
    if (id) void loadEvents().catch(toastError);
  }

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    if (!model) {
      showAgents = true;
      toast("No agent is ready on the remote yet. Install or sign in to one first.", true);
      return;
    }
    sending = true;
    try {
      const r = await remote.send(selected ?? "new", text, model);
      input = "";
      if (!selected) pick(r.id);
      await poll();
    } catch (e) {
      toastError(e);
    } finally {
      sending = false;
    }
  }

  async function stop() {
    if (!selected) return;
    try {
      await remote.kill(selected);
      await poll();
    } catch (e) {
      toastError(e);
    }
  }

  async function answer(a: RemoteApproval, allow: boolean) {
    try {
      await remote.answer(a.key, allow);
      approvals = approvals.filter((x) => x.key !== a.key);
      reschedule(300);
    } catch (e) {
      toastError(e);
    }
  }

  async function removeSession(s: SessionMeta) {
    const ok = await ask(`Delete "${s.title || "Untitled"}" on ${info?.label}?`, { title: "Delete remote session", kind: "warning" }).catch(() => false);
    if (!ok) return;
    try {
      await remote.remove(s.id);
      if (selected === s.id) pick(null);
      await poll();
    } catch (e) {
      toastError(e);
    }
  }

  async function agentAction(provider: string, action: "install" | "login") {
    try {
      await remote.agent(provider, action);
      toast(action === "install" ? "Installing in a terminal. Check again when it is done." : "Sign in in the terminal, then check again.");
    } catch (e) {
      toastError(e);
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  function stateText(a: ProviderStatus): string {
    switch (a.state) {
      case "ready":
        return a.account ? `Ready · ${a.account}` : "Ready";
      case "signed_out":
        return "Not signed in";
      case "not_installed":
        return "Not installed";
      case "disabled":
        return "Switched off";
      case "error":
        return a.hint || "Error";
      default:
        return "Installed";
    }
  }
</script>

{#if !loaded}
  <div class="empty"><span class="sub">Loading…</span></div>
{:else if !info}
  <div class="empty">
    <h3>No remote yet</h3>
    <p class="sub">Run Parzi on your own Linux server and keep working while this PC sleeps.</p>
    <button class="btn primary" on:click={() => dispatch("setup")}>Set up a remote</button>
  </div>
{:else}
  <div class="remote">
    <aside>
      <div class="head">
        <span class="dot" class:ok={reachable === true} class:bad={reachable === false} aria-hidden="true" />
        <span class="where" title={info.label}>{info.label}</span>
      </div>
      <button class="new" class:active={selected === null} on:click={() => pick(null)}>
        <Icon name="plus" size={14} />
        <span>New session</span>
      </button>
      <div class="list">
        {#each sessions as s (s.id)}
          <div class="row" class:active={s.id === selected}>
            <button class="pick" on:click={() => pick(s.id)} title={s.title || "Untitled"}>
              <span class="title">{s.title || "Untitled"}</span>
              <span class="meta">
                {#if approvals.some((a) => a.session === s.id)}<span class="needs">Needs you</span>{:else}{s.status}{/if}
              </span>
            </button>
            <button class="icon-btn" title="Delete" aria-label="Delete session" on:click={() => removeSession(s)}>
              <Icon name="trash" size={13} />
            </button>
          </div>
        {:else}
          <p class="sub pad">{reachable === false ? "Sessions show here once it answers." : reachable ? "No sessions on the remote yet." : "Loading…"}</p>
        {/each}
      </div>
      <div class="foot">
        <button class="link" on:click={() => (showAgents = !showAgents)}>{showAgents ? "Hide agents" : "Agents"}</button>
        <button class="link" on:click={() => dispatch("setup")}>Set up again</button>
      </div>
    </aside>

    <section>
      {#if reachable === false}
        <div class="banner bad">
          <span class="grow">Can't reach {info.label}: {reason}</span>
          <button class="btn" on:click={() => void poll()}>Try again</button>
        </div>
      {/if}

      {#if showAgents}
        <div class="agents">
          <div class="agents-head">
            <span class="grow">Agents on {info.host}</span>
            <button class="btn" disabled={checkingAgents} on:click={() => checkAgents(true)}>{checkingAgents ? "Checking…" : "Check again"}</button>
          </div>
          {#each agents as a (a.provider)}
            <div class="agent">
              <span class="grow">
                <span class="title">{nameOf(a.provider)}</span>
                <span class="sub" class:ok={a.state === "ready"}>{stateText(a)}</span>
              </span>
              {#if a.state === "not_installed"}
                <button class="btn" on:click={() => agentAction(a.provider, "install")}>Install</button>
              {:else if a.state !== "ready" && a.state !== "disabled"}
                <button class="btn" on:click={() => agentAction(a.provider, "login")}>Sign in</button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <div class="scroll">
        {#if selected}
          <div class="thread-col">
            <Thread {events} streaming={busy} />
          </div>
        {:else}
          <div class="empty small">
            <p class="sub">A new session runs on {info.host}, in its own scratch folder. Its approvals wait here until you answer.</p>
          </div>
        {/if}
      </div>

      {#each waiting as a (a.key)}
        <div class="banner">
          <span class="grow"><strong>Approve?</strong> {a.label}</span>
          <button class="btn" on:click={() => answer(a, false)}>Deny</button>
          <button class="btn primary" on:click={() => answer(a, true)}>Allow</button>
        </div>
      {/each}

      <div class="composer">
        <textarea
          rows="2"
          placeholder={selected ? "Reply on the remote" : `Start a session on ${info.host}`}
          bind:value={input}
          on:keydown={keydown}
        />
        <div class="bar">
          <select bind:value={model} disabled={!choices.length} aria-label="Agent and model">
            {#each choices as c (c.value)}
              <option value={c.value}>{c.label}</option>
            {:else}
              <option value="">No agent ready</option>
            {/each}
          </select>
          <span class="grow" />
          {#if busy}
            <button class="btn" on:click={stop}>Stop</button>
          {/if}
          <button class="btn primary" disabled={!input.trim() || sending} on:click={send}>{sending ? "Sending…" : "Send"}</button>
        </div>
      </div>
    </section>
  </div>
{/if}

<style>
  .remote {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }
  aside {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px;
    font-weight: 600;
  }
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--faint);
    flex: none;
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.bad {
    background: var(--bad);
  }
  .new {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 8px 6px;
    padding: 7px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--text);
    cursor: pointer;
  }
  .new:hover,
  .new.active {
    border-color: var(--accent);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 6px;
  }
  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .row:hover,
  .row.active {
    background: var(--line);
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 7px 8px;
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  .title {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 11.5px;
    color: var(--muted);
  }
  .needs {
    color: var(--warn);
    font-weight: 600;
  }
  .icon-btn {
    opacity: 0;
    background: none;
    border: none;
    color: var(--muted);
    padding: 6px;
    cursor: pointer;
    border-radius: var(--radius);
  }
  .row:hover .icon-btn,
  .icon-btn:focus-visible {
    opacity: 1;
  }
  .icon-btn:hover {
    color: var(--bad);
  }
  .foot {
    display: flex;
    justify-content: space-between;
    padding: 8px 12px;
    border-top: 1px solid var(--line);
  }
  .link {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 12.5px;
    padding: 2px 0;
  }
  .link:hover {
    color: var(--text);
  }
  section {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 24px;
  }
  .thread-col {
    max-width: 820px;
    margin: 0 auto;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 16px 0;
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
  }
  .banner.bad {
    border-color: var(--bad);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .agents {
    margin: 12px 16px 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    padding: 6px 12px;
  }
  .agents-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    font-weight: 600;
  }
  .agent {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    border-top: 1px solid var(--line);
  }
  .agent .grow {
    display: flex;
    flex-direction: column;
  }
  .sub {
    color: var(--muted);
    font-size: 12.5px;
  }
  .sub.ok {
    color: var(--ok);
  }
  .pad {
    padding: 8px;
  }
  .composer {
    margin: 10px 16px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    padding: 8px 10px;
  }
  .composer:focus-within {
    border-color: var(--accent);
  }
  textarea {
    width: 100%;
    resize: none;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font: inherit;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  select {
    max-width: 280px;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 4px 6px;
    font-size: 12.5px;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    padding: 24px;
  }
  .empty.small {
    min-height: 160px;
  }
  .empty h3 {
    margin: 0;
  }
  button:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
</style>
