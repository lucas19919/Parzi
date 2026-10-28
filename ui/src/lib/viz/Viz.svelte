<script lang="ts">
  // Draws a viz spec (lib/viz/spec.ts): a ```viz block in a thread, or a
  // custom widget on Home. A spec it can't draw says why, in one line.
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

  // The block's JSON text, or the parsed object.
  export let spec: unknown;
  // On a Home card: no frame and no title (the card has both), shorter charts.
  export let bare = false;

  $: parsed = parseViz(spec);
  $: s = parsed.ok ? parsed.spec : null;
  $: title = s?.title ?? "";
</script>

{#if s}
  <figure class="viz" class:bare data-viz={s.type} aria-label={bare || !title ? title || undefined : undefined}>
    {#if title && !bare}<figcaption>{title}</figcaption>{/if}
    {#if s.type === "stat"}
      <div class="stats">
        {#each s.stats as st}<Stat stat={st} unit={s.unit} />{/each}
      </div>
    {:else if s.type === "bars"}
      <Bars items={s.items} orient={s.orient} unit={s.unit} label={title} height={bare ? 112 : 168} />
    {:else if s.type === "hist"}
      <Hist bins={s.bins} total={s.total} mean={s.mean} unit={s.unit} label={title} height={bare ? 112 : 168} />
    {:else if s.type === "line"}
      <Line labels={s.labels} series={s.series} area={s.area} points={s.points} unit={s.unit} label={title} height={bare ? 112 : 180} />
    {:else if s.type === "spark"}
      <div class="spark-row">
        <Spark values={s.values} area={s.area} unit={s.unit} label={title} height={bare ? 40 : 32} />
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
  <div class="viz bad" class:bare data-viz="error" role="note">
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
    margin: var(--space) 0;
    padding: calc(var(--space) * 4) calc(var(--space) * 4);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--glass-bg);
    line-height: 1.4;
  }
  .viz.bare {
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
  }
  figcaption {
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
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
  .bad {
    gap: var(--space);
    padding: calc(var(--space) * 3) calc(var(--space) * 3);
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
    color: var(--warn);
  }
  summary {
    display: inline-flex;
    align-items: center;
    height: 24px;
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
