<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ThinkingFace from "./ThinkingFace.svelte";
  import { api, onRunEvent, type SessionMeta, type ShellJob } from "./api";
  import { activity } from "./activity";
  import { sessionTone, TONE_WORDS } from "./planModel";
  import { tooltip } from "./tooltip";

  /** Everything working beside this session: other sessions it started, and its commands. */
  export let session = "";
  export let helpers: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let waiting: Set<string> = new Set();

  const dispatch = createEventDispatcher<{ openSession: { id: string } }>();

  let jobs: ShellJob[] = [];
  let loadedFor = "";
  let now = Date.now();
  let showDone = false;

  $: if (session !== loadedFor) {
    loadedFor = session;
    jobs = [];
    void load();
  }
  async function load() {
    const sid = session;
    const got = sid ? await api.shellJobs(sid).catch(() => [] as ShellJob[]) : [];
    if (sid === session) jobs = got;
  }

  $: activeJobs = jobs.filter((j) => j.running);
  $: doneJobs = jobs.filter((j) => !j.running).reverse();
  $: busy = helpers.filter((h) => running.has(h.id)).length + activeJobs.length;

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
      if (activeJobs.length && ++tick % 3 === 0) void load();
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

<InfoCard id="running" title="Running">
  <svelte:fragment slot="aside">{busy ? `${busy} running` : ""}</svelte:fragment>
  {#if !helpers.length && !jobs.length}
    <div class="info-line"><span class="info-v quiet">Nothing running</span></div>
  {/if}
  {#each helpers as h (h.id)}
    {@const tone = sessionTone(h, running, waiting)}
    <button class="info-line row" use:tooltip={TONE_WORDS[tone]} on:click={() => dispatch("openSession", { id: h.id })}>
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
  {/each}
  {#each activeJobs as j (j.id)}
    <div class="info-line job">
      <Icon name="terminal" size={12} />
      <span class="info-v mono" use:tooltip={j.cmd}>{j.cmd}</span>
      <span class="info-time">{span(took(j))}</span>
      {#if j.background}
        <button class="stop" aria-label="Stop {j.cmd}" use:tooltip={"Stop"} on:click={() => stop(j)}><span class="square" /></button>
      {/if}
    </div>
  {/each}
  {#if doneJobs.length}
    <button class="info-line row quiet" aria-expanded={showDone} on:click={() => (showDone = !showDone)}>
      <Icon name="checkCircle" size={12} />
      <span class="info-v">{doneJobs.length} finished</span>
      <span class="chev" class:open={showDone}><Icon name="chevDown" size={12} /></span>
    </button>
    {#if showDone}
      {#each doneJobs as j (j.id)}
        <div class="info-line job quiet">
          <span class="status" class:bad={j.exit !== 0}><Icon name={j.exit === 0 ? "check" : "xCircle"} size={12} /></span>
          <span class="info-v mono" use:tooltip={j.exit === 0 ? j.cmd : `${j.cmd} (exit ${j.exit})`}>{j.cmd}</span>
          <span class="info-time">{span(j.ms)}</span>
        </div>
      {/each}
    {/if}
  {/if}
</InfoCard>

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
