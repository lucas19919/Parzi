<script lang="ts" context="module">
  // Charts already shown this run do not animate in again when the thread
  // draws them a second time.
  const seen = new Set<string>();
  const reduced = () => {
    try {
      return matchMedia("(prefers-reduced-motion: reduce)").matches;
    } catch {
      return true;
    }
  };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { tweened } from "svelte/motion";
  import { cubicOut } from "svelte/easing";
  import Icon from "../Icon.svelte";
  import Bars from "./Bars.svelte";
  import Hist from "./Hist.svelte";
  import Line from "./Line.svelte";
  import List from "./List.svelte";
  import Progress from "./Progress.svelte";
  import Spark from "./Spark.svelte";
  import Stat from "./Stat.svelte";
  import Table from "./Table.svelte";
  import Timeline from "./Timeline.svelte";
  import { fmt, parseViz } from "./spec";
  import { lerpNumbers, numbersOf, shapeOf, withNumbers } from "./tween";

  export let spec: unknown;

  $: parsed = parseViz(spec);
  $: real = parsed.ok ? parsed.spec : null;
  $: title = real?.title ?? "";

  // Entry: bars, histograms, progress and stat numbers grow from nothing,
  // lines and sparks draw on, rows rise in turn, once the chart is in view.
  // Updates: when only its numbers change, the chart eases to the new ones.
  const key = typeof spec === "string" ? spec : JSON.stringify(spec);
  const still = reduced() || seen.has(key);
  const grow = tweened(still ? 1 : 0, { duration: 900, easing: cubicOut });
  let waiting = !still;
  let rising = false;
  let figure: HTMLElement | null = null;

  const nums = tweened<number[]>([], { duration: 600, easing: cubicOut, interpolate: (a, b) => (t) => lerpNumbers(a, b, t) });
  let shape = "";
  $: if (real) {
    const next = shapeOf(real);
    void nums.set(numbersOf(real), { duration: shape && next === shape && !reduced() ? 600 : 0 });
    shape = next;
  }
  $: target = real ? numbersOf(real) : [];
  $: s = real ? withNumbers(real, $nums.length === target.length ? $nums : target, $grow) : null;

  function reveal() {
    seen.add(key);
    waiting = false;
    rising = true;
    void grow.set(1);
  }

  onMount(() => {
    if (!waiting) return;
    if (!figure || typeof IntersectionObserver === "undefined") {
      reveal();
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          reveal();
        }
      },
      { threshold: 0.2 },
    );
    io.observe(figure);
    return () => io.disconnect();
  });
</script>

{#if s}
  <figure bind:this={figure} class="viz" class:waiting class:rising data-viz={s.type} aria-label={!title ? title || undefined : undefined}>
    {#if title}<figcaption class="t-section">{title}</figcaption>{/if}
    {#if s.type === "stat"}
      <div class="stats">
        {#each s.stats as st}<Stat stat={st} unit={s.unit} draw={$grow} />{/each}
      </div>
    {:else if s.type === "bars"}
      <Bars items={s.items} orient={s.orient} unit={s.unit} label={title} height={168} grow={$grow} />
    {:else if s.type === "hist"}
      <Hist bins={s.bins} total={s.total} mean={s.mean} unit={s.unit} label={title} height={168} grow={$grow} />
    {:else if s.type === "line"}
      <Line labels={s.labels} series={s.series} area={s.area} points={s.points} unit={s.unit} label={title} height={180} draw={$grow} />
    {:else if s.type === "spark"}
      <div class="spark-row">
        <Spark values={s.values} area={s.area} unit={s.unit} label={title} height={32} draw={$grow} />
        <span class="last">{fmt(s.values[s.values.length - 1], s.unit)}</span>
      </div>
    {:else if s.type === "progress"}
      <Progress items={s.items} />
    {:else if s.type === "table"}
      <Table columns={s.columns} rows={s.rows} numeric={s.numeric} more={s.more} label={title} />
    {:else if s.type === "list"}
      <List items={s.items} more={s.more} label={title} />
    {:else if s.type === "timeline"}
      <Timeline items={s.items} more={s.more} label={title} />
    {/if}
    {#if s.more > 0 && (s.type === "bars" || s.type === "line" || s.type === "stat" || s.type === "progress")}
      <p class="more">{s.more} more {s.type === "line" ? "series" : "items"} not shown</p>
    {/if}
  </figure>
{:else if !parsed.ok}
  <div class="viz bad" data-viz="error" role="note">
    <p class="why"><Icon name="alert" size={12} /><span>{parsed.error}</span></p>
    <details>
      <summary>Source</summary>
      <pre>{parsed.source.slice(0, 4000)}</pre>
    </details>
  </div>
{/if}

<style>
  .viz {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
    min-width: 0;
    margin: 1em 0;
    padding: 0;
    border: none;
    background: transparent;
    line-height: 1.4;
  }
  .viz.bad {
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
  }
  /* Rows of lists, tables and timelines rise in turn the first time the
     chart is in view. Reduced motion skips it (theme.css). */
  .viz.waiting :global(.rise) {
    opacity: 0;
  }
  .viz.rising :global(.rise) {
    animation: -global-viz-rise 420ms var(--ease-out) both;
    animation-delay: calc(min(var(--i, 0), 14) * 45ms);
  }
  @keyframes -global-viz-rise {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(128px, 1fr));
    gap: calc(var(--space) * 4) calc(var(--space) * 6);
  }
  .spark-row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
  }
  .last {
    flex: none;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .more {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .why {
    display: flex;
    align-items: flex-start;
    gap: calc(var(--space) * 2);
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .why :global(svg) {
    margin-top: var(--space);
    color: var(--warn-text);
  }
  summary {
    display: inline-flex;
    align-items: center;
    height: var(--h-sm);
    font-size: var(--fs-sm);
    color: var(--muted);
    cursor: pointer;
  }
  summary:hover {
    color: var(--text);
  }
  pre {
    max-height: 200px;
    margin: var(--space) 0 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    overflow: auto;
    background: var(--bg);
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
