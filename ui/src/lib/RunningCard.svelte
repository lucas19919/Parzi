<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ThinkingFace from "./ThinkingFace.svelte";
  import { api, onRunEvent, type SessionMeta, type ShellJob } from "./api";
  import { activity, runLogs } from "./activity";
  import { sessionTone, TONE_WORDS } from "./sessionTone";
  import { tooltip } from "./tooltip";
  import type { LogStep } from "./runLog";
  import type { Notice } from "./turns";
  import { duration } from "./format";

  /** What the agent is doing in this session, oldest first: the steps of its
   *  latest run, the commands it ran, the helper sessions it started, and
   *  the thread's notices and messages from other sessions. */
  export let session = "";
  export let helpers: SessionMeta[] = [];
  export let notices: Notice[] = [];
  export let running: Set<string> = new Set();
  export let waiting: Set<string> = new Set();

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();
  /** Rows shown before "earlier" folds the rest away. */
  const SHOWN = 8;

  let jobs: ShellJob[] = [];
  let loadedFor = "";
  let now = Date.now();
  let showAll = false;

  $: if (session !== loadedFor) {
    loadedFor = session;
    jobs = [];
    showAll = false;
    void load();
  }
  async function load() {
    const sid = session;
    const got = sid ? await api.shellJobs(sid).catch(() => [] as ShellJob[]) : [];
    if (sid === session) jobs = got;
  }

  type Row =
    | { key: string; at: number; kind: "step"; step: LogStep }
    | { key: string; at: number; kind: "helper"; helper: SessionMeta }
    | { key: string; at: number; kind: "job"; job: ShellJob }
    | { key: string; at: number; kind: "note"; note: Notice };

  $: steps = $runLogs.get(session)?.steps ?? [];
  $: commands = jobs.filter((j) => !j.host_id);
  $: rows = [
    ...steps.map((s): Row => ({ key: `s:${s.id}`, at: s.at, kind: "step", step: s })),
    ...helpers.map((h): Row => ({ key: `h:${h.id}`, at: Date.parse(h.created) || 0, kind: "helper", helper: h })),
    ...commands.map((j): Row => ({ key: `j:${j.id}`, at: j.started, kind: "job", job: j })),
    ...notices.map((n): Row => ({ key: `n:${n.key}`, at: n.at, kind: "note", note: n })),
  ].sort((a, b) => a.at - b.at);
  $: earlier = showAll ? 0 : Math.max(0, rows.length - SHOWN);
  $: busy = helpers.filter((h) => running.has(h.id)).length + commands.filter((j) => j.running).length + steps.filter((s) => s.running).length;

  const took = (j: ShellJob) => (j.running ? Math.max(j.ms, now - j.started) : j.ms);
  function span(ms: number) {
    const s = Math.round(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ${s % 60}s`;
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }

  /** What a subagent is on (its current step), or what it reported. */
  function detail(h: SessionMeta, tone: string, act: Map<string, { label: string }>): string {
    if (tone === "need") return "Needs you";
    if (tone === "live") return act.get(h.id)?.label ?? "";
    return (h.last_run?.summary ?? "").split("\n")[0].trim();
  }

  async function stop(j: ShellJob) {
    if (await api.shellKill(session, j.id).catch(() => false)) void load();
  }

  onMount(() => {
    let tick = 0;
    const timer = setInterval(() => {
      now = Date.now();
      if (commands.some((j) => j.running) && ++tick % 2 === 0) void load();
    }, 1000);
    const un = onRunEvent((e) => {
      if (e.session !== session) return;
      if (e.kind === "tool_call" || e.kind === "tool_result" || e.kind === "done" || e.kind === "error") void load();
    });
    return () => {
      clearInterval(timer);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<!-- Nothing done yet, nothing to list: the card is gone. -->
{#if rows.length}
<InfoCard id="running" title="Running">
  <svelte:fragment slot="aside">{busy ? `${busy} running` : ""}</svelte:fragment>
  {#if earlier || showAll}
    <button class="info-line row quiet" aria-expanded={showAll} on:click={() => (showAll = !showAll)}>
      <span class="chev" class:open={showAll}><Icon name="chevDown" size={12} /></span>
      <span class="info-v">{showAll ? "Show fewer" : `${earlier} earlier`}</span>
    </button>
  {/if}
  {#each rows.slice(earlier) as r (r.key)}
    {#if r.kind === "step"}
      {@const s = r.step}
      <div class="info-line" class:quiet={!s.running}>
        {#if s.running}
          <i class="info-dot t-live" />
        {:else}
          <span class="status" class:bad={!s.ok}><Icon name={s.ok ? "check" : "xCircle"} size={12} /></span>
        {/if}
        <span class="info-v" use:tooltip={s.ok || s.running ? s.label : `${s.label} (failed)`}>{s.label}</span>
        <span class="info-time">{s.running ? span(now - s.at) : s.ms ? duration(s.ms) : ""}</span>
      </div>
    {:else if r.kind === "helper"}
      {@const h = r.helper}
      {@const tone = sessionTone(h, running, waiting)}
      <button class="info-line row" use:tooltip={detail(h, tone, $activity) ? `${TONE_WORDS[tone]}: ${detail(h, tone, $activity)}` : TONE_WORDS[tone]} on:click={() => dispatch("openSession", { id: h.id })}>
        {#if tone === "live"}
          <ThinkingFace size={16} />
        {:else}
          <i class="info-dot t-{tone}" />
        {/if}
        <span class="helper">
          <span class="info-v">{h.title || "Untitled"}</span>
          {#if detail(h, tone, $activity)}<span class="detail">{detail(h, tone, $activity)}</span>{/if}
        </span>
      </button>
    {:else if r.kind === "note"}
      {@const n = r.note}
      {#if n.from}
        <button class="info-line row quiet" use:tooltip={n.tip} on:click={() => dispatch("openSession", { id: n.from })}>
          <Icon name="chat" size={12} />
          <span class="info-v">{n.text}</span>
        </button>
      {:else}
        <div class="info-line quiet" use:tooltip={n.tip}>
          <i class="info-dot" />
          <span class="info-v">{n.text}</span>
        </div>
      {/if}
    {:else}
      {@const j = r.job}
      <div class="info-line job" class:quiet={!j.running}>
        {#if j.running}
          <Icon name="terminal" size={12} />
        {:else}
          <span class="status" class:bad={j.exit !== 0}><Icon name={j.exit === 0 ? "check" : "xCircle"} size={12} /></span>
        {/if}
        <span class="info-v mono" use:tooltip={j.exit === 0 || j.running ? j.cmd : `${j.cmd} (exit ${j.exit})`}>{j.cmd}</span>
        <span class="info-time">{span(took(j))}</span>
        {#if j.running && j.background}
          <button class="stop" aria-label="Stop {j.cmd}" use:tooltip={"Stop"} on:click={() => stop(j)}><span class="square" /></button>
        {/if}
      </div>
    {/if}
  {/each}
</InfoCard>
{/if}

<style>
  .helper {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
    text-align: left;
  }
  .detail {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .stop {
    width: 20px;
    height: 20px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    cursor: pointer;
  }
  .stop:hover {
    background: var(--hover);
  }
  .square {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-sm);
    background: var(--muted);
  }
  .stop:hover .square {
    background: var(--text);
  }
  .status.bad :global(svg) {
    color: var(--bad-text);
  }
  .chev {
    display: inline-flex;
    transform: rotate(-90deg);
    transition: transform 160ms var(--ease-out);
  }
  .chev.open {
    transform: none;
  }
  .info-dot.t-live {
    background: var(--accent);
  }
  .info-dot.t-need {
    background: var(--warn);
  }
  .info-dot.t-bad {
    background: var(--bad);
  }
  .info-dot.t-ok {
    background: var(--ok);
  }
</style>
