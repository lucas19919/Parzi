<script lang="ts" context="module">
  // The rail's geometry: left inset, lane pitch, the node's radius and its
  // centre from the row top (level with the title's first line), and how far
  // a connector takes to bend between a lane and a node.
  export const RAIL = { pad: 8, lane: 8, r: 5, y: 14, bend: 10 };

  export function spineX(lanes: number): number {
    return RAIL.pad + lanes * RAIL.lane + RAIL.r;
  }

  export function railWidth(lanes: number): number {
    return spineX(lanes) + RAIL.r + 8;
  }
</script>

<script lang="ts">
  // One row of the plan's rail: the spine, connectors passing by or joining
  // here, and the node (none for a phase heading).
  import type { RailRow } from "./planModel";

  export let row: RailRow;
  export let lanes = 0;
  // The node's look: an item state, or "" for a row without a node.
  export let node = "";
  // Checks on the item's open PR, which colour its ring.
  export let checks = "";

  $: S = spineX(lanes);
  $: W = railWidth(lanes);
  const { y: Y, bend: B } = RAIL;
  // Lane 0 runs next to the spine, the others further left.
  $: X = (lane: number) => S - (lane + 1) * RAIL.lane;
</script>

<svg class="rail" width={W} aria-hidden="true">
  {#if node ? row.up : row.up && row.down}
    <line class="spine" x1={S} y1="0" x2={S} y2={node ? Y : "100%"} />
  {/if}
  {#if node && row.down}
    <line class="spine" x1={S} y1={Y} x2={S} y2="100%" />
  {/if}
  {#each row.through as l (l)}
    <line class="need" x1={X(l)} y1="0" x2={X(l)} y2="100%" />
  {/each}
  {#each row.into as l (l.lane)}
    <line class="need" x1={X(l.lane)} y1="0" x2={X(l.lane)} y2={l.ends ? Y - B : "100%"} />
    <path class="need" d="M {X(l.lane)} {Y - B} C {X(l.lane)} {Y - B * 0.3} {S - B * 0.7} {Y} {S} {Y}" />
  {/each}
  {#if row.out >= 0}
    <path class="need" d="M {S} {Y} C {S - B * 0.7} {Y} {X(row.out)} {Y + B * 0.3} {X(row.out)} {Y + B}" />
    <line class="need" x1={X(row.out)} y1={Y + B} x2={X(row.out)} y2="100%" />
  {/if}
  {#if node === "working"}<circle class="halo" cx={S} cy={Y} r="9" />{/if}
  {#if node}
    <circle class="node n-{node} c-{checks}" cx={S} cy={Y} r={RAIL.r} />
    {#if node === "started"}<circle class="pip" cx={S} cy={Y} r="1.75" />{/if}
  {/if}
</svg>

<style>
  .rail {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }
  .spine {
    stroke: var(--line);
    stroke-width: 1.5;
  }
  .need {
    fill: none;
    stroke: color-mix(in srgb, var(--muted) 55%, transparent);
    stroke-width: 1.5;
  }
  .node {
    fill: var(--panel);
    stroke: var(--faint);
    stroke-width: 1.5;
  }
  .n-started {
    stroke: var(--muted);
  }
  .pip {
    fill: var(--muted);
  }
  .n-review {
    stroke: var(--ok);
    stroke-width: 2;
  }
  .n-review.c-fail {
    stroke: var(--bad);
  }
  .n-review.c-pending {
    stroke: var(--muted);
  }
  .n-working {
    fill: var(--ok);
    stroke: var(--ok);
  }
  .n-needs {
    fill: var(--warn);
    stroke: var(--warn);
  }
  .n-done {
    fill: var(--muted);
    stroke: var(--muted);
  }
  .n-dropped {
    stroke-dasharray: 2 2;
  }
  .halo {
    fill: color-mix(in srgb, var(--ok) 24%, transparent);
    transform-box: fill-box;
    transform-origin: center;
    animation: halo 1.6s var(--ease-out) infinite;
  }
  @keyframes halo {
    50% {
      opacity: 0.2;
      transform: scale(1.25);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .halo {
      animation: none;
    }
  }
</style>
