<script lang="ts">
  // The Work tab: every goal the agents are on, by what it needs from you,
  // with a goal's page beside the list when one is open.
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import GoalCard from "./GoalCard.svelte";
  import GoalPage from "./GoalPage.svelte";
  import { brain, type Project, type SessionMeta } from "./api";
  import { persisted } from "./browserData";
  import { covered } from "./overlay";
  import { refreshWork, targetGoal, work, workError, workLoaded } from "./workStore";
  import { ALL, BANDS, engineMatch, groupBands, inProject, otherEngines, projectOptions, type EngineFilter } from "./workModel";

  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let followUp: (session: string, text: string) => Promise<boolean> = async () => false;

  const dispatch = createEventDispatcher<{ openSession: { id: string }; newSession: void }>();

  const ENGINES: { id: EngineFilter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "here", label: "This PC" },
    { id: "server", label: "Server" },
  ];

  const scope = persisted<string>("parzi.work.scope.v1", ALL);
  const engine = persisted<EngineFilter>("parzi.work.engine.v1", "all");
  let projects: Project[] = [];
  let selected = "";

  $: titles = new Map(projects.map((p) => [p.slug.toLowerCase(), p.title]));
  $: options = projectOptions($work.goals, projects);
  // A remembered project without work any more falls back to All.
  $: if ($workLoaded && !options.some((o) => o.value === $scope)) scope.set(ALL);
  $: showEngines = otherEngines($work.goals, !!$work.server?.linked);
  $: engineNow = showEngines ? $engine : "all";
  $: shown = $work.goals.filter((g) => inProject(g, $scope) && engineMatch(g, engineNow));
  $: bands = groupBands(shown, $work.asks);
  $: total = BANDS.reduce((n, b) => n + bands[b.id].length, 0);
  $: anyWork = $work.goals.some((g) => g.status !== "archived");
  $: counts = [
    bands.needs.length ? `${bands.needs.length} need${bands.needs.length === 1 ? "s" : ""} you` : "",
    bands.working.length ? `${bands.working.length} working` : "",
  ].filter(Boolean);

  onMount(() => {
    void refreshWork();
    brain
      .projects()
      .then((p) => (projects = p))
      .catch(() => {});
    // Escape closes the goal page. Capture phase, so App doesn't also read
    // it as "stop the run"; the switcher and menus (covered) come first.
    const onEsc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !selected || e.defaultPrevented || $covered) return;
      e.preventDefault();
      selected = "";
    };
    window.addEventListener("keydown", onEsc, true);
    return () => window.removeEventListener("keydown", onEsc, true);
  });

  // Opened at a goal (a toast, Home, the dock, the session strip).
  $: if ($targetGoal && $workLoaded) void reveal($targetGoal);

  async function reveal(id: string) {
    targetGoal.set("");
    const g = $work.goals.find((x) => x.id === id);
    if (g && !inProject(g, $scope)) scope.set(ALL);
    if (g && !engineMatch(g, engineNow)) engine.set("all");
    selected = id;
    await tick();
    document.querySelector(`[data-goal="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function open(id: string) {
    selected = selected === id ? "" : id;
  }
</script>

<div class="work" class:split={!!selected} data-project={$scope} data-engine={engineNow}>
  <!-- Nothing to filter before there is work. -->
  {#if anyWork || !$workLoaded}
  <header class="bar">
    <label class="chip">
      <Icon name="project" size={12} />
      <select aria-label="Project" bind:value={$scope}>
        {#each options as o (o.value)}
          <option value={o.value}>{o.label}{o.count ? ` (${o.count})` : ""}</option>
        {/each}
      </select>
      <Icon name="chevDown" size={12} />
    </label>
    {#if showEngines}
      <div class="seg engines" role="group" aria-label="Where it runs" data-ui-segmented>
        {#each ENGINES as e (e.id)}
          <button aria-pressed={engineNow === e.id} class:on={engineNow === e.id} on:click={() => engine.set(e.id)}>{e.label}</button>
        {/each}
      </div>
    {/if}
    <span class="sum">{counts.join(" · ")}</span>
  </header>
  {/if}

  {#if $workError}
    <p class="err">Couldn't load work: {$workError}</p>
  {/if}

  <div class="body">
    <div class="list">
      {#if !anyWork}
        {#if $workLoaded && !$workError}
          <div class="blank">
            <span class="blank-icon"><Icon name="goal" /></span>
            <h2>No work yet</h2>
            <p>Ask for something in the composer. Real work shows up here; chats stay in History.</p>
            <button class="btn" on:click={() => dispatch("newSession")}><Icon name="plus" size={12} />New session</button>
          </div>
        {/if}
      {:else if !total}
        <p class="none">Nothing here for this filter.</p>
      {:else}
        {#each BANDS as b (b.id)}
          {#if bands[b.id].length}
            <section class="band {b.id}" aria-labelledby="band-{b.id}">
              <h3 id="band-{b.id}"><i class="bdot" />{b.label}<span class="n">{bands[b.id].length}</span></h3>
              <div class="cards">
                {#each bands[b.id] as g (g.id)}
                  <GoalCard goal={g} selected={selected === g.id} showEngine={showEngines} on:open={(e) => open(e.detail.id)} />
                {/each}
              </div>
            </section>
          {/if}
        {/each}
      {/if}
    </div>

    {#if selected}
      <GoalPage
        id={selected}
        {threads}
        {running}
        {followUp}
        projectTitles={titles}
        on:close={() => (selected = "")}
        on:openSession={(e) => dispatch("openSession", { id: e.detail.id })}
      />
    {/if}
  </div>
</div>

<style>
  .work {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 20px;
    border-bottom: 1px solid var(--line);
  }
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
  }
  .chip:hover {
    background: var(--hover);
    color: var(--text);
  }
  .chip select {
    max-width: 220px;
    height: 26px;
    padding: 0 2px;
    background: none;
    border: none;
    color: var(--text);
    font-size: var(--fs-sm);
    appearance: none;
    cursor: pointer;
  }
  .chip select:focus {
    box-shadow: none;
  }
  .chip select option {
    background: var(--panel);
    color: var(--text);
  }
  .seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--text) 5%, transparent);
  }
  .seg button {
    height: 24px;
    padding: 0 12px;
    background: none;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    color: var(--text);
  }
  .sum {
    margin-left: auto;
    font-size: var(--fs-sm);
    color: var(--muted);
    white-space: nowrap;
  }
  .err {
    margin: 8px 20px 0;
    font-size: var(--fs-md);
    color: var(--bad);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .list {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 16px 20px 24px;
  }
  .split .list {
    flex: none;
    width: clamp(300px, 34%, 400px);
    padding: 16px 16px 24px;
  }
  .band + .band {
    margin-top: 22px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .bdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .needs .bdot {
    background: var(--warn);
  }
  .working .bdot {
    background: var(--ok);
  }
  .n {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 10px;
  }
  .split .cards {
    grid-template-columns: minmax(0, 1fr);
  }
  .none {
    margin: 8px 0;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .blank {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    max-width: 380px;
    margin: 12vh auto 0;
    text-align: center;
  }
  .blank-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--hover);
    color: var(--muted);
  }
  .blank h2 {
    margin: 4px 0 0;
    font-size: var(--fs-lg);
    font-weight: 600;
  }
  .blank p {
    margin: 0;
    font-size: var(--fs-md);
    line-height: 1.55;
    color: var(--muted);
  }
  .blank .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
  }
</style>
