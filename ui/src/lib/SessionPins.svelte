<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { api, onRunEvent, type HostJob, type ProjectPlan, type SessionMeta } from "./api";
  import { sessionPins, targetsSummary, type Pin } from "./pins";
  import { coalesce } from "./threadList";
  import { toastError } from "./toast";
  import { tooltip } from "./tooltip";

  /** One quiet row above the composer: dev servers, the session's live plan,
   *  and the project's targets, with interactive popovers. */
  export let session = "";
  export let project = "";
  export let sessionPlan: SessionMeta["plan"] = null;
  export let streaming = false;

  const dispatch = createEventDispatcher<{ open: Pin }>();

  let hosts: HostJob[] = [];
  let plan: ProjectPlan | null = null;
  let key = "";
  let openPop: "plan" | "targets" | null = null;
  let wrapEl: HTMLDivElement | null = null;

  $: planItems = sessionPlan?.items ?? [];
  $: planDoc = sessionPlan?.doc ?? null;
  $: planDone = planItems.filter((i) => i.state === "done").length;
  $: pins = sessionPins(hosts, planDoc, planItems);
  $: summary = targetsSummary(plan);
  $: pendingHostUrl = hosts.some((h) => h.running && h.urls.length === 0);
  $: anyRunningHost = hosts.some((h) => h.running);

  $: targetsTip = [
    plan?.goal ?? "",
    ...(summary?.open ?? []).map((t) => `${t.working ? "Working: " : ""}${t.title}`),
    "Click to view project targets",
  ]
    .filter(Boolean)
    .join("\n");

  const reload = coalesce(async () => {
    const [sid, slug] = [session, project];
    const [h, p] = await Promise.all([
      sid ? api.shellHosts(sid).catch(() => [] as HostJob[]) : [],
      slug ? api.projectPlan(slug).catch(() => null) : null,
    ]);
    if (sid !== session || slug !== project) return;
    hosts = h;
    plan = p;
  }, 250);

  $: if (`${session}\n${project}` !== key) {
    key = `${session}\n${project}`;
    hosts = [];
    plan = null;
    openPop = null;
    void reload();
  }

  // Dev servers often print their localhost URL a second or two after shell_start
  // returns. Poll gently while a server is booting or running so pins stay live.
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  function clearPoll() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }
  $: {
    clearPoll();
    if (session && (pendingHostUrl || streaming)) {
      pollTimer = setInterval(() => void reload(), 2000);
    } else if (session && anyRunningHost) {
      pollTimer = setInterval(() => void reload(), 6000);
    }
  }

  function clickPin(p: Pin) {
    if (p.kind === "plan") {
      openPop = openPop === "plan" ? null : "plan";
      return;
    }
    openPop = null;
    dispatch("open", p);
  }

  function openPlanDoc() {
    const doc = planDoc?.trim();
    if (!doc) return;
    openPop = null;
    dispatch("open", { kind: "file", label: "Plan", target: doc });
  }

  async function openProjectPlanDoc() {
    openPop = null;
    try {
      const path = await api.projectPlanDoc(project);
      if (path) dispatch("open", { kind: "file", label: "Project plan", target: path });
    } catch (e) {
      toastError(e);
    }
  }

  function onWindowPointerDown(e: PointerEvent) {
    if (openPop && wrapEl && !wrapEl.contains(e.target as Node)) {
      openPop = null;
    }
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (openPop && e.key === "Escape") {
      openPop = null;
    }
  }

  onMount(() => {
    const un = onRunEvent((e) => {
      if (e.session !== session) return;
      if (
        e.kind === "tool_result" ||
        e.kind === "done" ||
        e.kind === "error" ||
        e.kind === "paused" ||
        e.kind === "refreshed"
      ) {
        void reload();
      }
    });
    return () => {
      clearPoll();
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<svelte:window on:pointerdown={onWindowPointerDown} on:keydown={onWindowKeydown} />

{#if pins.length || summary}
  <div class="pins-wrap" bind:this={wrapEl}>
    {#if openPop === "plan" && planItems.length}
      <div class="popover" role="dialog" aria-label="Session plan">
        <div class="pop-head">
          <span class="pop-title">Session plan</span>
          <span class="pop-badge">{planDone}/{planItems.length}</span>
          {#if planDoc?.trim()}
            <button class="pop-link" on:click={openPlanDoc}>Open doc</button>
          {/if}
        </div>
        {#if sessionPlan?.summary}
          <p class="pop-sub">{sessionPlan.summary}</p>
        {/if}
        <ul class="pop-list">
          {#each planItems as item, i (i)}
            <li class="pop-item {item.state}">
              <span class="mark {item.state}" aria-hidden="true">
                {#if item.state === "done"}
                  <Icon name="check" size={12} />
                {:else if item.state === "active"}
                  <i class="dot" />
                {:else}
                  <i class="ring" />
                {/if}
              </span>
              <span class="pop-text">{item.title}</span>
            </li>
          {/each}
        </ul>
      </div>
    {:else if openPop === "targets" && summary}
      <div class="popover" role="dialog" aria-label="Project targets">
        <div class="pop-head">
          <span class="pop-title">Project targets</span>
          <span class="pop-badge">{summary.done}/{summary.done + summary.open.length}</span>
          <button class="pop-link" on:click={openProjectPlanDoc}>Open plan</button>
        </div>
        {#if plan?.goal}
          <p class="pop-sub">{plan.goal}</p>
        {/if}
        {#if summary.open.length}
          <ul class="pop-list">
            {#each summary.open as t, i (i)}
              <li class="pop-item" class:active={t.working}>
                <span class="mark" class:active={t.working} aria-hidden="true">
                  {#if t.working}
                    <i class="dot" />
                  {:else}
                    <i class="ring" />
                  {/if}
                </span>
                <span class="pop-text">{t.title}</span>
                {#if t.working}<span class="tag">Working</span>{/if}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="pop-empty">All {summary.done} targets done.</p>
        {/if}
      </div>
    {/if}

    <div class="pins">
      {#each pins as p (p.kind + ":" + p.target)}
        {@const active = p.kind === "plan" && openPop === "plan"}
        <button
          class="pin"
          class:on={active}
          aria-pressed={active}
          aria-expanded={p.kind === "plan" ? openPop === "plan" : undefined}
          use:tooltip={p.kind === "plan" ? "Click to view session plan" : p.target}
          on:click={() => clickPin(p)}
        >
          <Icon name={p.kind === "url" ? "globe" : "plan"} size={12} />
          {#if p.kind === "plan" && planItems.some((i) => i.state === "active")}
            <i class="working" aria-hidden="true" />
          {/if}
          <span class="text">{p.label}</span>
        </button>
      {/each}
      {#if summary}
        <button
          class="pin targets"
          class:on={openPop === "targets"}
          aria-expanded={openPop === "targets"}
          use:tooltip={targetsTip}
          on:click={() => (openPop = openPop === "targets" ? null : "targets")}
        >
          <Icon name="project" size={12} />
          {#if summary.open.some((t) => t.working)}<i class="working" aria-label="Being worked on" />{/if}
          <span class="text">{summary.done}/{summary.done + summary.open.length}</span>
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .pins-wrap {
    position: relative;
    min-width: 0;
  }
  .popover {
    position: absolute;
    left: var(--space);
    bottom: calc(100% + var(--space) * 1.5);
    z-index: 30;
    width: min(440px, calc(100% - var(--space) * 2));
    max-height: 300px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 3);
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: 0 10px 28px color-mix(in srgb, var(--scrim) 55%, transparent);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .pop-head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  .pop-title {
    font-weight: 600;
    color: var(--text);
  }
  .pop-badge {
    padding: calc(var(--space) * 0.5) calc(var(--space) * 1.5);
    background: var(--fill);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }
  .pop-link {
    margin-left: auto;
    padding: calc(var(--space) * 0.5) calc(var(--space) * 2);
    background: transparent;
    border: 1px solid var(--edge);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-xs);
    cursor: pointer;
    transition: background var(--dur) ease, color var(--dur) ease;
  }
  .pop-link:hover {
    background: var(--fill);
    color: var(--text);
  }
  .pop-sub,
  .pop-empty {
    margin: 0;
    color: var(--muted);
    line-height: 1.5;
  }
  .pop-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 1.5);
  }
  .pop-item {
    display: flex;
    align-items: flex-start;
    gap: calc(var(--space) * 2);
    line-height: 1.45;
  }
  .pop-item.done {
    color: var(--muted);
  }
  .pop-item.active .pop-text {
    color: var(--text);
    font-weight: 500;
  }
  .mark {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: calc(var(--space) * 0.5);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--ok-text);
  }
  .dot {
    display: block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .ring {
    display: block;
    width: 10px;
    height: 10px;
    box-sizing: border-box;
    border: 1.5px solid var(--faint);
    border-radius: 50%;
  }
  .pop-text {
    flex: 1;
    min-width: 0;
  }
  .tag {
    flex: none;
    padding: 0 calc(var(--space) * 1.5);
    background: var(--fill);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-xs);
  }
  .pins {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: calc(var(--space) * 1.5);
    min-width: 0;
    margin: 0 0 var(--space) var(--space);
  }
  .pin {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space);
    max-width: 220px;
    min-width: 0;
    height: var(--h-sm);
    padding: 0 calc(var(--space) * 2);
    background: var(--fill);
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    transition: background var(--dur) var(--ease-out), color var(--dur) var(--ease-out), border-color var(--dur) var(--ease-out);
  }
  .pin:hover,
  .pin.on {
    background: var(--hover);
    border-color: var(--line);
    color: var(--text);
  }
  .pin:active {
    transform: scale(0.97);
  }
  .pin :global(svg) {
    flex: none;
  }
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
  .working {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    flex: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .pin {
      transition: none;
    }
  }
</style>
