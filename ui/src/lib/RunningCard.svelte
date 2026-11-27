<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { api, onRunEvent, type SessionMeta, type ShellJob } from "./api";
  import { activity, runLogs } from "./activity";
  import { sessionTone, TONE_WORDS } from "./sessionTone";
  import { tooltip } from "./tooltip";
  import type { LogStep } from "./runLog";
  import { duration } from "./format";
  import { commandIntent, isShellStepLabel, stepIntent } from "./intentLabel";

  /** What the agent is doing in this session, oldest first: the steps of its
   *  latest run, the commands it ran and the helper sessions it started.
   *  One plain mark each: a spinner while it runs, a check or a cross when
   *  done, the agent mark for a helper. */
  export let session = "";
  export let helpers: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let waiting: Set<string> = new Set();

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();
  /** Rows shown before "earlier" folds the rest away. */
  const SHOWN = 6;

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
    | { key: string; at: number; kind: "job"; job: ShellJob };

  $: rawSteps = $runLogs.get(session)?.steps ?? [];
  $: commands = jobs.filter((j) => !j.host_id);
  // Dedupe shell tool steps that already have a richer ShellJob row at the same time.
  $: steps = rawSteps.filter((s) => {
    if (!isShellStepLabel(s.label)) return true;
    return !commands.some((j) => Math.abs(j.started - s.at) < 3000);
  });
  $: rows = [
    ...steps.map((s): Row => ({ key: `s:${s.id}`, at: s.at, kind: "step", step: s })),
    ...helpers.map((h): Row => ({ key: `h:${h.id}`, at: Date.parse(h.created) || 0, kind: "helper", helper: h })),
    ...commands.map((j): Row => ({ key: `j:${j.id}`, at: j.started, kind: "job", job: j })),
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
    if (tone === "live") {
      const raw = act.get(h.id)?.label ?? "";
      return raw ? stepIntent(raw, true) : "";
    }
    return (h.last_run?.summary ?? "").split("\n")[0].trim();
  }

  async function stop(j: ShellJob) {
    if (await api.shellKill(session, j.id).catch(() => false)) void load();
  }

  let loadTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleLoad() {
    if (loadTimer) return;
    loadTimer = setTimeout(() => {
      loadTimer = undefined;
      void load();
    }, 250);
  }

  onMount(() => {
    let tick = 0;
    const timer = setInterval(() => {
      if (document.hidden || busy === 0) return;
      now = Date.now();
      if (commands.some((j) => j.running) && ++tick % 2 === 0) void load();
    }, 1000);
    const un = onRunEvent((e) => {
      if (e.session !== session) return;
      if (e.kind === "done" || e.kind === "error") void load();
      else if (e.kind === "tool_call" || e.kind === "tool_result") scheduleLoad();
    });
    return () => {
      clearInterval(timer);
      if (loadTimer) clearTimeout(loadTimer);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<!-- Nothing done yet, nothing to list: the card is gone. -->
{#if rows.length}
<InfoCard id="running" title="Running">
  <svelte:fragment slot="aside">{busy ? `${busy} running` : ""}</svelte:fragment>
  {#if earlier || (showAll && rows.length > SHOWN)}
    <button class="info-line row quiet" aria-expanded={showAll} on:click={() => (showAll = !showAll)}>
      <span class="chev" class:open={showAll}><Icon name="chevDown" size={12} /></span>
      <span class="info-v">{showAll ? "Show fewer" : `${earlier} earlier`}</span>
    </button>
  {/if}
  {#each rows.slice(earlier) as r (r.key)}
    {#if r.kind === "step"}
      {@const s = r.step}
      {@const short = stepIntent(s.label, s.running)}
      <div class="info-line" class:quiet={!s.running}>
        {#if s.running}
          <span class="mark"><svg class="spin" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.5" /></svg></span>
        {:else}
          <span class="mark"><Icon name={s.ok ? "check" : "close"} size={12} /></span>
        {/if}
        <span class="info-v" use:tooltip={s.ok || s.running ? s.label : `${s.label} (failed)`}>{short}</span>
        <span class="info-time">{s.running ? span(now - s.at) : s.ms ? duration(s.ms) : ""}</span>
      </div>
    {:else if r.kind === "helper"}
      {@const h = r.helper}
      {@const tone = sessionTone(h, running, waiting)}
      <button class="info-line row" class:quiet={tone !== "live" && tone !== "need"} use:tooltip={detail(h, tone, $activity) ? `${TONE_WORDS[tone]}: ${detail(h, tone, $activity)}` : TONE_WORDS[tone]} on:click={() => dispatch("openSession", { id: h.id })}>
        <span class="mark"><Icon name="bot" size={12} /></span>
        <span class="info-v">{h.title || "Untitled"}</span>
      </button>
    {:else}
      {@const j = r.job}
      {@const short = commandIntent(j.cmd, j.urls, j.running)}
      <div class="info-line job" class:quiet={!j.running}>
        {#if j.running}
          <span class="mark"><svg class="spin" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.5" /></svg></span>
        {:else}
          <span class="mark"><Icon name={j.exit === 0 ? "check" : "close"} size={12} /></span>
        {/if}
        <span class="info-v" use:tooltip={j.exit === 0 || j.running ? j.cmd : `${j.cmd} (exit ${j.exit})`}>{short}</span>
        <span class="info-time">{span(took(j))}</span>
        {#if j.running && j.background}
          <button class="stop" aria-label="Stop {short}" use:tooltip={`Stop: ${j.cmd}`} on:click={() => stop(j)}><span class="square" /></button>
        {/if}
      </div>
    {/if}
  {/each}
</InfoCard>
{/if}

<style>
  .mark {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 12px;
    height: 12px;
  }
  .spin {
    width: 12px;
    height: 12px;
    animation: spin 0.9s linear infinite;
  }
  .spin circle {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-dasharray: 18 10;
    stroke-linecap: round;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
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
  .chev {
    display: inline-flex;
    transform: rotate(-90deg);
    transition: transform 160ms var(--ease-out);
  }
  .chev.open {
    transform: none;
  }
</style>
