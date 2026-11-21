<script lang="ts">
  import { BUTTON, frameSize, lightInk, screenAt, type Art, type Phone } from "./devices";

  /** The phone to draw around the page, or null to pass the page through. */
  export let phone: Phone | null;
  /** How far the phone is scaled down (devices.ts phoneFit). */
  export let fit = 1;
  /** The page's colour: the status bar and home indicator are painted in it. */
  export let bg = "rgb(255, 255, 255)";

  // Side buttons, at full size: which edge, how far along the body, how long.
  type Button = { side: "left" | "right" | "top"; at: number; len: number; camera?: boolean };
  const BUTTONS: Record<Art, Button[]> = {
    // Action button and volume on the left; side button and Camera Control on the right.
    iphone: [
      { side: "left", at: 150, len: 34 },
      { side: "left", at: 222, len: 62 },
      { side: "left", at: 300, len: 62 },
      { side: "right", at: 232, len: 100 },
      { side: "right", at: 548, len: 58, camera: true },
    ],
    // Power above the volume rocker, both on the right.
    android: [
      { side: "right", at: 220, len: 70 },
      { side: "right", at: 320, len: 130 },
    ],
    "duo-closed": [
      { side: "left", at: 140, len: 30 },
      { side: "left", at: 200, len: 56 },
      { side: "left", at: 268, len: 56 },
      { side: "right", at: 220, len: 90 },
    ],
    // Open, the buttons of the right half sit on its top edge.
    "duo-open": [
      { side: "top", at: 470, len: 56 },
      { side: "top", at: 536, len: 56 },
      { side: "top", at: 640, len: 70 },
    ],
  };
  // Metal edge colours: black titanium, matte aluminium, silver.
  const METAL: Record<Art, string[]> = {
    iphone: ["#6a6c70", "#2b2c2f", "#55575b", "#1f2022", "#4d4f53"],
    android: ["#3c3d41", "#202124", "#333438", "#1b1c1e", "#2f3033"],
    "duo-open": ["#e6e7e9", "#a9acb0", "#d8d9dc", "#9a9da2", "#cfd1d4"],
    "duo-closed": ["#e6e7e9", "#a9acb0", "#d8d9dc", "#9a9da2", "#cfd1d4"],
  };

  $: p = phone;
  $: size = p ? frameSize(p) : { width: 0, height: 0 };
  $: s = p ? screenAt(p) : 0;
  $: bodyW = size.width - 2 * BUTTON;
  $: bodyH = size.height - 2 * BUTTON;
  $: ink = lightInk(bg) ? "#fff" : "#000";
  $: ios = p?.art !== "android";
  $: metal = p ? METAL[p.art] : [];
  $: cx = p ? s + p.width / 2 : 0;
  // The status bar's middle line, where the time, glyphs and camera sit.
  $: barY = p ? s + (p.art === "iphone" || p.art === "duo-closed" ? 29.5 : p.top / 2) : 0;
  $: ear = p ? (p.width - 126) / 2 : 0;
  $: timeX = !p ? 0 : p.art === "android" ? s + 24 : p.art === "duo-open" ? s + 30 : s + ear / 2 + 6;
  $: glyphsX = !p ? 0 : p.art === "android" ? s + p.width - 22 - 62 : p.art === "duo-open" ? s + p.width - 30 - 72 : s + p.width - ear / 2 - 6 - 36;
  $: pill = !p ? 0 : p.art === "android" ? 108 : p.art === "duo-open" ? 150 : p.art === "duo-closed" ? 128 : 140;
  $: id = p ? `ph-${p.art}` : "ph";

  function button(b: Button) {
    const out = BUTTON + 2;
    if (b.side === "top") return { x: BUTTON + b.at, y: 0, width: b.len, height: out };
    return { x: b.side === "left" ? 0 : BUTTON + bodyW - 2, y: BUTTON + b.at, width: out, height: b.len };
  }

  /** One wifi band as an arc over (x, y), radius r. */
  function arc(x: number, y: number, r: number) {
    const d = r * Math.SQRT1_2;
    return `M${x - d} ${y - d}A${r} ${r} 0 0 1 ${x + d} ${y - d}`;
  }
  function wedge(x: number, y: number, r: number) {
    const d = r * Math.SQRT1_2;
    return `M${x} ${y}L${x - d} ${y - d}A${r} ${r} 0 0 1 ${x + d} ${y - d}Z`;
  }
</script>

<div
  class="device"
  class:on={!!p}
  style:width={p ? `${size.width * fit}px` : null}
  style:height={p ? `${size.height * fit}px` : null}
>
  {#if p}
    <svg class="art" viewBox="0 0 {size.width} {size.height}" width={size.width * fit} height={size.height * fit} aria-hidden="true">
      <defs>
        <linearGradient id="{id}-metal" x1="0" y1="0" x2="1" y2="1">
          {#each metal as c, i}<stop offset={i / (metal.length - 1)} stop-color={c} />{/each}
        </linearGradient>
        <linearGradient id="{id}-btn-v" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stop-color={metal[3]} />
          <stop offset="0.45" stop-color={metal[0]} />
          <stop offset="1" stop-color={metal[1]} />
        </linearGradient>
        <linearGradient id="{id}-btn-h" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color={metal[3]} />
          <stop offset="0.45" stop-color={metal[0]} />
          <stop offset="1" stop-color={metal[1]} />
        </linearGradient>
        <linearGradient id="{id}-sheen" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="#fff" stop-opacity="0.09" />
          <stop offset="0.35" stop-color="#fff" stop-opacity="0" />
        </linearGradient>
        <filter id="{id}-shadow" x="-25%" y="-15%" width="150%" height="135%">
          <feDropShadow dx="0" dy="18" stdDeviation="22" flood-color="#000" flood-opacity="0.32" />
        </filter>
      </defs>

      <g filter="url(#{id}-shadow)">
        {#each BUTTONS[p.art] as b}
          {@const r = button(b)}
          <rect {...r} rx="1.6" fill="url(#{id}-btn-{b.side === 'top' ? 'h' : 'v'})" />
        {/each}
        <rect x={BUTTON} y={BUTTON} width={bodyW} height={bodyH} rx={p.radius + p.bezel + p.edge} fill="url(#{id}-metal)" stroke="#000" stroke-opacity="0.45" stroke-width="0.6" />
      </g>
      <!-- Light catching the metal's outer and inner rims. -->
      <rect x={BUTTON + 0.5} y={BUTTON + 0.5} width={bodyW - 1} height={bodyH - 1} rx={p.radius + p.bezel + p.edge - 0.5} fill="none" stroke="#fff" stroke-opacity="0.22" />
      {#each BUTTONS[p.art].filter((b) => b.camera) as b}
        <!-- Camera Control: sapphire, nearly flush, in a metal ring. -->
        <rect x={BUTTON + bodyW - 1.5} y={BUTTON + b.at + 2} width="2.5" height={b.len - 4} rx="1" fill="#141517" />
      {/each}
      <rect
        x={BUTTON + p.edge}
        y={BUTTON + p.edge}
        width={bodyW - 2 * p.edge}
        height={bodyH - 2 * p.edge}
        rx={p.radius + p.bezel}
        fill="#050506"
        stroke="#000"
        stroke-opacity="0.7"
      />
      <rect x={BUTTON + p.edge} y={BUTTON + p.edge} width={bodyW - 2 * p.edge} height={bodyH - 2 * p.edge} rx={p.radius + p.bezel} fill="url(#{id}-sheen)" />
      {#if p.art === "duo-open"}
        <!-- The hinge, where the edge bends at the hinge. -->
        <rect x={cx - 1} y={BUTTON} width="2" height={p.edge} fill="#000" fill-opacity="0.25" />
        <rect x={cx - 1} y={BUTTON + bodyH - p.edge} width="2" height={p.edge} fill="#000" fill-opacity="0.25" />
      {/if}

      <!-- The screen, in the page's colour; the page itself covers its middle. -->
      <rect x={s} y={s} width={p.width} height={p.height} rx={p.radius} fill={bg} />

      {#if p.art === "duo-open"}
        <!-- The crease, seen only in the bars: the page covers the rest. -->
        <rect x={cx - 5} y={s} width="10" height={p.height} fill={ink} fill-opacity="0.03" />
        <line x1={cx} y1={s} x2={cx} y2={s + p.height} stroke={ink} stroke-opacity="0.08" />
      {/if}

      <g fill={ink}>
        <text
          x={timeX}
          y={barY + (ios ? 6 : 5)}
          text-anchor={p.art === "iphone" || p.art === "duo-closed" ? "middle" : "start"}
          font-family={ios ? "-apple-system, 'SF Pro Text', Inter, system-ui, sans-serif" : "Roboto, 'Google Sans', Inter, system-ui, sans-serif"}
          font-size={ios ? 17 : 14}
          font-weight={ios ? 600 : 500}
          letter-spacing={ios ? -0.3 : 0.2}>9:41</text
        >
        {#if ios}
          <g transform="translate({glyphsX} {barY - 6})">
            {#each [4, 6.5, 9, 11.5] as h, i}<rect x={i * 4.5} y={12 - h} width="3" height={h} rx="1" />{/each}
            <path d={arc(30, 11.5, 10)} fill="none" stroke={ink} stroke-width="2.2" stroke-linecap="round" />
            <path d={arc(30, 11.5, 6.4)} fill="none" stroke={ink} stroke-width="2.2" stroke-linecap="round" />
            <path d={wedge(30, 11.8, 3.6)} stroke={ink} stroke-width="1" stroke-linejoin="round" />
            <rect x="44.5" y="0.5" width="24" height="11" rx="3.6" fill="none" stroke={ink} stroke-opacity="0.38" />
            <rect x="46.5" y="2.5" width="17" height="7" rx="1.8" />
            <rect x="69.5" y="4" width="1.6" height="4" rx="0.8" fill-opacity="0.4" />
          </g>
        {:else}
          <g transform="translate({glyphsX} {barY - 6.5})">
            <path d={wedge(7.5, 12, 10.5)} />
            <path d="M34 0.5V13H21.5Z" stroke={ink} stroke-linejoin="round" />
            <rect x="40" y="1.5" width="20" height="10" rx="3" fill-opacity="0.35" />
            <rect x="40" y="1.5" width="15" height="10" rx="3" />
            <rect x="60.5" y="4.5" width="1.5" height="4" rx="0.7" fill-opacity="0.35" />
          </g>
        {/if}
      </g>

      {#if p.art === "iphone" || p.art === "duo-closed"}
        <!-- The Dynamic Island, with the front camera's lens at its right. -->
        <rect x={cx - 63} y={s + 11} width="126" height="37" rx="18.5" fill="#000" />
        <circle cx={cx + 44} cy={s + 29.5} r="5.5" fill="#0b0d12" />
        <circle cx={cx + 42.8} cy={s + 28.3} r="1.6" fill="#1f2738" />
      {:else}
        <!-- A punch-hole camera: centred on the Pixel, in the right half on the open Duo. -->
        {@const hx = p.art === "android" ? cx : s + p.width * 0.75}
        <circle cx={hx} cy={barY} r={p.art === "android" ? 10 : 8} fill="#000" />
        <circle cx={hx} cy={barY} r="4" fill="#0b0d12" />
        <circle cx={hx - 1.2} cy={barY - 1.2} r="1.2" fill="#232b3c" />
      {/if}

      <rect
        x={cx - pill / 2}
        y={p.art === "android" ? s + p.height - 14 : s + p.height - 13}
        width={pill}
        height={p.art === "android" ? 4 : 5}
        rx={p.art === "android" ? 2 : 2.5}
        fill={ink}
        fill-opacity="0.9"
      />
    </svg>
  {/if}
  <div class="screen" class:on={!!p} style:left={p ? `${s * fit}px` : null} style:top={p ? `${(s + p.top) * fit}px` : null}>
    <slot />
  </div>
</div>

<style>
  .device,
  .screen {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }
  .device.on {
    flex: none;
    position: relative;
  }
  .art {
    position: absolute;
    inset: 0;
    overflow: visible;
    pointer-events: none;
  }
  .screen.on {
    position: absolute;
  }
</style>
