<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { api, onRunEvent, type ShellJob } from "./api";
  import { tooltip } from "./tooltip";

  export let session = "";

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

  $: running = jobs.filter((j) => j.running);
  $: done = jobs.filter((j) => !j.running).reverse();

  const took = (j: ShellJob) => (j.running ? Math.max(j.ms, now - j.started) : j.ms);
  function span(ms: number) {
    const s = Math.round(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ${s % 60}s`;
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }

  async function stop(j: ShellJob) {
    if (await api.shellKill(session, j.id).catch(() => false)) void load();
  }

  onMount(() => {
    let tick = 0;
    const timer = setInterval(() => {
      now = Date.now();
      if (running.length && ++tick % 3 === 0) void load();
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

  <InfoCard id="jobs" title="Background tasks">
    <svelte:fragment slot="aside">{running.length ? `${running.length} running` : done.length ? `${done.length} finished` : ""}</svelte:fragment>
    {#if !jobs.length}
      <div class="info-line"><span class="info-v quiet">No background tasks</span></div>
    {/if}
    {#each running as j (j.id)}
      <div class="info-line job">
        <Icon name="terminal" size={12} />
        <span class="info-v mono" use:tooltip={j.cmd}>{j.cmd}</span>
        <span class="info-time">{span(took(j))}</span>
        {#if j.background}
          <button class="stop" aria-label="Stop {j.cmd}" use:tooltip={"Stop"} on:click={() => stop(j)}><span class="square" /></button>
        {/if}
      </div>
    {/each}
    {#if done.length}
      <button class="info-line row quiet" aria-expanded={showDone} on:click={() => (showDone = !showDone)}>
        <Icon name="checkCircle" size={12} />
        <span class="info-v">{done.length} finished</span>
        <span class="chev" class:open={showDone}><Icon name="chevDown" size={12} /></span>
      </button>
      {#if showDone}
        {#each done as j (j.id)}
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
    border-radius: var(--radius-sm);
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
</style>
