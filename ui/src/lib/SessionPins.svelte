<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { api, onRunEvent, type HostJob, type ProjectPlan } from "./api";
  import { sessionPins, targetsSummary, type Pin } from "./pins";
  import { coalesce } from "./threadList";
  import { toastError } from "./toast";
  import { tooltip } from "./tooltip";

  /** One quiet row above the composer: what the session keeps coming back
   *  to (its dev servers, its plan), then the project's targets. */
  export let session = "";
  export let project = "";
  export let planDoc: string | null = null;
  /** The pin open over the thread now, if any. */
  export let current = "";

  const dispatch = createEventDispatcher<{ open: Pin }>();

  let hosts: HostJob[] = [];
  let plan: ProjectPlan | null = null;
  let key = "";

  $: pins = sessionPins(hosts, planDoc);
  $: summary = targetsSummary(plan);
  // The row ends in an ellipsis; the tooltip has the goal and every open target.
  $: targetsTip = [plan?.goal ?? "", ...(summary?.open ?? []).map((t) => `${t.working ? "Working: " : ""}${t.title}`), "Click to open the project plan"]
    .filter(Boolean)
    .join("\n");

  const reload = coalesce(async () => {
    const [sid, slug] = [session, project];
    const [h, p] = await Promise.all([
      sid ? api.shellHosts(sid).catch(() => [] as HostJob[]) : [],
      // Until the engine has project plans the command is missing: no row.
      slug ? api.projectPlan(slug).catch(() => null) : null,
    ]);
    if (sid !== session || slug !== project) return;
    hosts = h;
    plan = p;
  }, 300);

  $: if (`${session}\n${project}` !== key) {
    key = `${session}\n${project}`;
    hosts = [];
    plan = null;
    void reload();
  }

  async function openPlan() {
    try {
      const path = await api.projectPlanDoc(project);
      if (path) dispatch("open", { kind: "file", label: "Project plan", target: path });
    } catch (e) {
      toastError(e);
    }
  }

  onMount(() => {
    // Servers start and targets move while the agent works: look again when
    // a step ends or the run does, never on a timer.
    const un = onRunEvent((e) => {
      if (e.session !== session) return;
      if (e.kind === "tool_result" || e.kind === "done" || e.kind === "error" || e.kind === "paused" || e.kind === "refreshed") void reload();
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

{#if pins.length || summary}
  <div class="pins">
    {#each pins as p (p.target)}
      <button class="pin" class:on={current === p.target} aria-pressed={current === p.target} use:tooltip={p.target} on:click={() => dispatch("open", p)}>
        <Icon name={p.kind === "url" ? "globe" : "plan"} size={12} />
        <span class="text">{p.label}</span>
      </button>
    {/each}
    {#if summary}
      <button class="pin targets" use:tooltip={targetsTip} on:click={openPlan}>
        <Icon name="project" size={12} />
        <span class="text">
          {#each summary.open as t, i}
            {#if i}<span class="sep">·</span>{/if}
            {#if t.working}<i class="working" aria-label="Being worked on" />{/if}{t.title}
          {/each}
          {#if summary.done}{#if summary.open.length}<span class="sep">·</span>{/if}<span class="done">{summary.done} done</span>{/if}
        </span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .pins {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
    margin: 0 0 var(--space) var(--space);
  }
  .pin {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space);
    max-width: 200px;
    min-width: 0;
    height: var(--h-sm);
    padding: 0 calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    transition: background var(--dur) var(--ease-out), color var(--dur) var(--ease-out);
  }
  .pin:hover,
  .pin.on {
    background: var(--hover);
    color: var(--text);
  }
  .pin:active {
    transform: scale(0.97);
  }
  .pin :global(svg) {
    flex: none;
  }
  /* The targets take what room is left and end in an ellipsis. */
  .targets {
    flex: 0 1 auto;
    max-width: none;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sep {
    margin: 0 var(--space);
  }
  .working {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: var(--space);
    border-radius: 50%;
    background: var(--accent);
    vertical-align: middle;
  }
  @media (prefers-reduced-motion: reduce) {
    .pin {
      transition: none;
    }
  }
</style>
