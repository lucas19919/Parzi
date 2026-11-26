<script lang="ts">
  import { BUTTON, frameSize, lightInk, screenOrigin, type Art, type Phone } from "./devices";

  /** The phone to draw around the page, or null to pass the page through. */
  export let phone: Phone | null;
  /** How far the phone is scaled down (devices.ts phoneFit). */
  export let fit = 1;
  /** The page's colour: the status bar, right rail and home indicator are painted in it. */
  export let bg = "rgb(255, 255, 255)";

  // Hardware buttons at full size: which edge, offset along the body, length, and finish.
  type Button = { side: "left" | "right" | "top"; at: number; len: number; camera?: boolean; touchId?: boolean };
  const BUTTONS: Record<Art, Button[]> = {
    // iPhone 16 Pro: Action + volume on left; side button + Camera Control on right.
    iphone: [
      { side: "left", at: 150, len: 34 },
      { side: "left", at: 222, len: 62 },
      { side: "left", at: 300, len: 62 },
      { side: "right", at: 232, len: 100 },
      { side: "right", at: 548, len: 58, camera: true },
    ],
    // Pixel 9: power above the volume rocker, both on the right.
    android: [
      { side: "right", at: 220, len: 70 },
      { side: "right", at: 320, len: 130 },
    ],
    // iPhone Duo open (landscape): volume on top-right; Touch ID side button + Camera Control on right.
    "duo-open": [
      { side: "top", at: 556, len: 54 },
      { side: "top", at: 624, len: 54 },
      { side: "right", at: 122, len: 86, touchId: true },
      { side: "right", at: 468, len: 58, camera: true },
    ],
    // iPhone Duo open (portrait, rotated 90 deg CCW): volume on upper-left; Touch ID + Camera Control on top.
    "duo-open-portrait": [
      { side: "left", at: 313, len: 54 },
      { side: "left", at: 381, len: 54 },
      { side: "top", at: 122, len: 86, touchId: true },
      { side: "top", at: 468, len: 58, camera: true },
    ],
    // iPhone Duo closed: titanium hinge spine on left (no left buttons); volume on top;
    // Touch ID side button + Camera Control on right.
    "duo-closed": [
      { side: "top", at: 120, len: 52 },
      { side: "top", at: 186, len: 52 },
      { side: "right", at: 146, len: 84, touchId: true },
      { side: "right", at: 468, len: 58, camera: true },
    ],
  };

  // Frame finishes: black titanium (iPhone), matte aluminium (Pixel), mirror-polished Grade 5 titanium (Duo).
  const DUO_METAL = ["#f2f3f5", "#9ea3ab", "#e5e7eb", "#848a94", "#d6d9de", "#a8adb6", "#eef0f2"];
  const METAL: Record<Art, string[]> = {
    iphone: ["#6a6c70", "#2b2c2f", "#55575b", "#1f2022", "#4d4f53"],
    android: ["#3c3d41", "#202124", "#333438", "#1b1c1e", "#2f3033"],
    "duo-open": DUO_METAL,
    "duo-open-portrait": DUO_METAL,
    "duo-closed": DUO_METAL,
  };

  $: p = phone;
  $: size = p ? frameSize(p) : { width: 0, height: 0 };
  $: origin = p ? screenOrigin(p) : { x: 0, y: 0 };
  $: sx = origin.x;
  $: sy = origin.y;
  $: bodyW = size.width - 2 * BUTTON;
  $: bodyH = size.height - 2 * BUTTON;
  $: ink = lightInk(bg) ? "#fff" : "#000";
  $: ios = p?.art !== "android";
  $: isDuo = !!p && p.art.startsWith("duo-");
  $: metal = p ? METAL[p.art] : [];
  $: cx = p ? sx + p.width / 2 : 0;
  $: cy = p ? sy + p.height / 2 : 0;
  $: id = p ? `ph-${p.art}` : "ph";

  // Corner radii for screen, black glass bezel, and titanium outer body (asymmetric on closed Duo).
  $: rScreenL = p ? (p.leftRadius ?? p.radius) : 0;
  $: rScreenR = p ? p.radius : 0;
  $: glassX = p ? (p.art === "duo-closed" ? BUTTON + (p.spine ?? 0) + p.edge + 1 : BUTTON + p.edge) : 0;
  $: glassW = p ? BUTTON + bodyW - p.edge - glassX : 0;
  $: glassY = p ? BUTTON + p.edge : 0;
  $: glassH = p ? bodyH - 2 * p.edge : 0;
  $: rGlassL = p ? (p.art === "duo-closed" ? rScreenL + 3 : rScreenL + p.bezel) : 0;
  $: rGlassR = p ? rScreenR + p.bezel : 0;
  $: rBodyL = p ? (p.art === "duo-closed" ? 16 : rGlassL + p.edge) : 0;
  $: rBodyR = p ? rGlassR + p.edge : 0;

  $: bodyPath = p ? rrect(BUTTON, BUTTON, bodyW, bodyH, rBodyL, rBodyR, rBodyR, rBodyL) : "";
  $: rimPath = p ? rrect(BUTTON + 0.5, BUTTON + 0.5, bodyW - 1, bodyH - 1, rBodyL - 0.5, rBodyR - 0.5, rBodyR - 0.5, rBodyL - 0.5) : "";
  $: glassPath = p ? rrect(glassX, glassY, glassW, glassH, rGlassL, rGlassR, rGlassR, rGlassL) : "";
  $: innerLipPath = p ? rrect(sx - 2, sy - 2, p.width + 4, p.height + 4, rScreenL + 2, rScreenR + 2, rScreenR + 2, rScreenL + 2) : "";
  $: screenPath = p ? rrect(sx, sy, p.width, p.height, rScreenL, rScreenR, rScreenR, rScreenL) : "";

  // Status bar layout for standard top-bar modes (iPhone, Android, Duo open portrait).
  $: barY = p ? sy + (p.art === "iphone" ? 29.5 : p.top / 2) : 0;
  $: ear = p ? (p.width - 126) / 2 : 0;
  $: timeX = !p ? 0 : p.art === "android" ? sx + 24 : p.art === "duo-open-portrait" ? sx + 48 : sx + ear / 2 + 6;
  $: glyphsX = !p
    ? 0
    : p.art === "android"
      ? sx + p.width - 22 - 62
      : p.art === "duo-open-portrait"
        ? sx + p.width - 48 - 72
        : sx + p.width - ear / 2 - 6 - 36;

  // Closed Duo 84pt right vertical rail center.
  $: railW = p?.right ?? 0;
  $: railX = p ? sx + p.width - railW / 2 : 0;

  // Home indicator pill width and center X (centered under the page viewport on closed Duo).
  $: pill = !p ? 0 : p.art === "android" ? 108 : p.art === "duo-open" ? 180 : p.art === "duo-open-portrait" ? 156 : p.art === "duo-closed" ? 116 : 140;
  $: pillCx = !p ? 0 : p.art === "duo-closed" ? sx + (p.width - railW) / 2 : cx;

  function rrect(x: number, y: number, w: number, h: number, tl: number, tr = tl, br = tl, bl = tl): string {
    return [
      `M${x + tl} ${y}`,
      `H${x + w - tr}`,
      `A${tr} ${tr} 0 0 1 ${x + w} ${y + tr}`,
      `V${y + h - br}`,
      `A${br} ${br} 0 0 1 ${x + w - br} ${y + h}`,
      `H${x + bl}`,
      `A${bl} ${bl} 0 0 1 ${x} ${y + h - bl}`,
      `V${y + tl}`,
      `A${tl} ${tl} 0 0 1 ${x + tl} ${y}`,
      "Z",
    ].join("");
  }

  function buttonRect(b: Button) {
    const out = BUTTON + 2;
    if (b.side === "top") return { x: BUTTON + b.at, y: 0, width: b.len, height: out };
    return { x: b.side === "left" ? 0 : BUTTON + bodyW - 2, y: BUTTON + b.at, width: out, height: b.len };
  }

  function insetFace(b: Button) {
    if (b.side === "top") return { x: BUTTON + b.at + 2, y: BUTTON - 1, width: b.len - 4, height: 2.5 };
    if (b.side === "left") return { x: BUTTON - 1, y: BUTTON + b.at + 2, width: 2.5, height: b.len - 4 };
    return { x: BUTTON + bodyW - 1.5, y: BUTTON + b.at + 2, width: 2.5, height: b.len - 4 };
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
          <stop offset="0" stop-color="#fff" stop-opacity="0.1" />
          <stop offset="0.32" stop-color="#fff" stop-opacity="0" />
        </linearGradient>
        {#if p.art === "duo-closed"}
          <!-- Mirror-polished cylindrical Grade 5 titanium hinge spine along the left edge. -->
          <linearGradient id="{id}-spine" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stop-color="#9096a0" />
            <stop offset="0.24" stop-color="#f7f8fa" />
            <stop offset="0.52" stop-color="#b4b9c2" />
            <stop offset="0.78" stop-color="#dfe2e7" />
            <stop offset="0.93" stop-color="#6e737c" />
            <stop offset="1" stop-color="#26282c" />
          </linearGradient>
          <clipPath id="{id}-body-clip">
            <path d={bodyPath} />
          </clipPath>
        {/if}
        {#if p.art === "duo-open"}
          <!-- Delicate nano-texture optical reflection along the vertical fold, visible only in the bars. -->
          <linearGradient id="{id}-crease" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stop-color={ink} stop-opacity="0" />
            <stop offset="0.36" stop-color={ink} stop-opacity="0.035" />
            <stop offset="0.5" stop-color="#fff" stop-opacity="0.05" />
            <stop offset="0.64" stop-color={ink} stop-opacity="0.03" />
            <stop offset="1" stop-color={ink} stop-opacity="0" />
          </linearGradient>
        {/if}
        <clipPath id="{id}-screen-clip">
          <path d={screenPath} />
        </clipPath>
        <filter id="{id}-shadow" x="-25%" y="-15%" width="150%" height="135%">
          <feDropShadow dx="0" dy="18" stdDeviation="22" flood-color="#000" flood-opacity="0.32" />
        </filter>
      </defs>

      <!-- Outer hardware buttons + titanium/aluminium body with drop shadow. -->
      <g filter="url(#{id}-shadow)">
        {#each BUTTONS[p.art] as b}
          {@const r = buttonRect(b)}
          <rect {...r} rx="1.6" fill="url(#{id}-btn-{b.side === 'top' ? 'h' : 'v'})" />
        {/each}
        <path d={bodyPath} fill="url(#{id}-metal)" stroke="#000" stroke-opacity="0.45" stroke-width="0.6" />
      </g>

      {#if p.art === "duo-closed"}
        <!-- Folded left titanium hinge spine with precision top/bottom end-cap seams. -->
        <g clip-path="url(#{id}-body-clip)">
          <rect x={BUTTON} y={BUTTON} width={(p.spine ?? 0) + p.edge + 1} height={bodyH} fill="url(#{id}-spine)" />
          <line x1={glassX - 0.5} y1={BUTTON} x2={glassX - 0.5} y2={BUTTON + bodyH} stroke="#000" stroke-opacity="0.55" stroke-width="0.8" />
          <line x1={BUTTON} y1={BUTTON + 18} x2={glassX - 1} y2={BUTTON + 18} stroke="#000" stroke-opacity="0.22" stroke-width="0.8" />
          <line x1={BUTTON} y1={BUTTON + bodyH - 18} x2={glassX - 1} y2={BUTTON + bodyH - 18} stroke="#000" stroke-opacity="0.22" stroke-width="0.8" />
        </g>
      {/if}

      <!-- Specular highlight catching the metal's outer rim. -->
      <path d={rimPath} fill="none" stroke="#fff" stroke-opacity={isDuo ? 0.42 : 0.22} />

      <!-- Specialized hardware button faces: flush sapphire Camera Control and Touch ID side sensor. -->
      {#each BUTTONS[p.art].filter((b) => b.camera || b.touchId) as b}
        {@const f = insetFace(b)}
        <rect
          {...f}
          rx="1"
          fill={b.camera ? "#141517" : "#cfd3da"}
          stroke={b.touchId ? "#5a5f68" : "none"}
          stroke-width={b.touchId ? 0.5 : 0}
        />
      {/each}

      <!-- Black glass bezel + diagonal glass sheen. -->
      <path d={glassPath} fill="#050506" stroke="#000" stroke-opacity="0.7" />
      <path d={glassPath} fill="url(#{id}-sheen)" />

      {#if p.art === "duo-open" || p.art === "duo-open-portrait"}
        <!-- Raised inner protective bezel lip around the flexible OLED display. -->
        <path d={innerLipPath} fill="none" stroke="#fff" stroke-opacity="0.1" stroke-width="1" />
      {/if}

      {#if p.art === "duo-open"}
        <!-- Precision titanium hinge caps at top and bottom of the vertical fold axis. -->
        <rect x={cx - 22} y={BUTTON - 0.5} width="44" height={p.edge + 3} rx="1.5" fill="url(#{id}-btn-v)" stroke="#000" stroke-opacity="0.32" stroke-width="0.6" />
        <rect x={cx - 22} y={BUTTON + bodyH - p.edge - 2.5} width="44" height={p.edge + 3} rx="1.5" fill="url(#{id}-btn-v)" stroke="#000" stroke-opacity="0.32" stroke-width="0.6" />
        <line x1={cx} y1={BUTTON} x2={cx} y2={BUTTON + p.edge + 2} stroke="#000" stroke-opacity="0.38" stroke-width="0.9" />
        <line x1={cx} y1={BUTTON + bodyH - p.edge - 2} x2={cx} y2={BUTTON + bodyH} stroke="#000" stroke-opacity="0.38" stroke-width="0.9" />
        <!-- Protective polymer T-caps cushioning the hinge ends on the black bezel. -->
        <rect x={cx - 10} y={BUTTON + p.edge} width="20" height="5" rx="1.5" fill="#141518" />
        <rect x={cx - 10} y={BUTTON + bodyH - p.edge - 5} width="20" height="5" rx="1.5" fill="#141518" />
      {:else if p.art === "duo-open-portrait"}
        <!-- Precision titanium hinge caps at left and right of the horizontal fold axis. -->
        <rect x={BUTTON - 0.5} y={cy - 22} width={p.edge + 3} height="44" rx="1.5" fill="url(#{id}-btn-h)" stroke="#000" stroke-opacity="0.32" stroke-width="0.6" />
        <rect x={BUTTON + bodyW - p.edge - 2.5} y={cy - 22} width={p.edge + 3} height="44" rx="1.5" fill="url(#{id}-btn-h)" stroke="#000" stroke-opacity="0.32" stroke-width="0.6" />
        <line x1={BUTTON} y1={cy} x2={BUTTON + p.edge + 2} y2={cy} stroke="#000" stroke-opacity="0.38" stroke-width="0.9" />
        <line x1={BUTTON + bodyW - p.edge - 2} y1={cy} x2={BUTTON + bodyW} y2={cy} stroke="#000" stroke-opacity="0.38" stroke-width="0.9" />
        <rect x={BUTTON + p.edge} y={cy - 10} width="5" height="20" rx="1.5" fill="#141518" />
        <rect x={BUTTON + bodyW - p.edge - 5} y={cy - 10} width="5" height="20" rx="1.5" fill="#141518" />
      {/if}

      <!-- The screen in the page's colour; the live webview covers the inner viewport. -->
      <path d={screenPath} fill={bg} />

      <g clip-path="url(#{id}-screen-clip)">
        {#if p.art === "duo-open"}
          <!-- Subtle nano-texture crease reflection in the top and bottom chrome bands. -->
          <rect x={cx - 14} y={sy} width="28" height={p.top} fill="url(#{id}-crease)" />
          <rect x={cx - 14} y={sy + p.height - p.bottom} width="28" height={p.bottom} fill="url(#{id}-crease)" />

          <!-- iOS 27 unfolded landscape status cluster: time + compact circular status emblem at top-right. -->
          <g fill={ink}>
            <text
              x={sx + p.width - 74}
              y={sy + 31.5}
              text-anchor="end"
              font-family="-apple-system, 'SF Pro Text', Inter, system-ui, sans-serif"
              font-size="16"
              font-weight="600"
              letter-spacing="-0.3">9:41</text
            >
            <g transform="translate({sx + p.width - 52} {sy + 26}) scale(0.76)">
              <circle cx="0" cy="0" r="19" fill={ink} fill-opacity="0.06" />
              <circle cx="0" cy="0" r="15.5" fill="none" stroke={ink} stroke-opacity="0.22" stroke-width="2.2" />
              <circle
                cx="0"
                cy="0"
                r="15.5"
                fill="none"
                stroke={ink}
                stroke-width="2.2"
                stroke-linecap="round"
                stroke-dasharray="76 98"
                transform="rotate(-90)"
              />
              <path d={arc(0, 2.5, 7.6)} fill="none" stroke={ink} stroke-width="1.8" stroke-linecap="round" />
              <path d={arc(0, 2.5, 4.5)} fill="none" stroke={ink} stroke-width="1.8" stroke-linecap="round" />
              <circle cx="0" cy="2.6" r="1.4" />
              <circle cx="-5.4" cy="8.4" r="1.2" />
              <circle cx="-1.8" cy="9.2" r="1.2" />
              <circle cx="1.8" cy="9.2" r="1.2" />
              <circle cx="5.4" cy="8.4" r="1.2" fill-opacity="0.35" />
            </g>
          </g>
        {:else if p.art === "duo-closed"}
          <!-- Safari 27.1 84pt vertical right rail on the outer display (clock, vertical Dynamic Island, circular status). -->
          <line x1={sx + p.width - railW + 0.5} y1={sy} x2={sx + p.width - railW + 0.5} y2={sy + p.height} stroke={ink} stroke-opacity="0.06" stroke-width="1" />

          <!-- Top-right vertical Dynamic Island around the 12MP Center Stage hole-punch camera. -->
          <rect x={railX - 18} y={sy + 28} width="36" height="88" rx="18" fill="#000" />
          <circle cx={railX} cy={sy + 48} r="7.5" fill="#0b0d12" />
          <circle cx={railX} cy={sy + 48} r="4.5" fill="#121824" />
          <circle cx={railX - 1.5} cy={sy + 46.5} r="1.5" fill="#314262" />

          <!-- Time + compact circular status emblem stacked directly below the vertical Dynamic Island. -->
          <g fill={ink}>
            <text
              x={railX}
              y={sy + 144}
              text-anchor="middle"
              font-family="-apple-system, 'SF Pro Text', Inter, system-ui, sans-serif"
              font-size="16"
              font-weight="600"
              letter-spacing="-0.3">9:41</text
            >
            <g transform="translate({railX} {sy + 184})">
              <circle cx="0" cy="0" r="19" fill={ink} fill-opacity="0.06" />
              <circle cx="0" cy="0" r="15.5" fill="none" stroke={ink} stroke-opacity="0.22" stroke-width="2.2" />
              <circle
                cx="0"
                cy="0"
                r="15.5"
                fill="none"
                stroke={ink}
                stroke-width="2.2"
                stroke-linecap="round"
                stroke-dasharray="76 98"
                transform="rotate(-90)"
              />
              <path d={arc(0, 2.5, 7.6)} fill="none" stroke={ink} stroke-width="1.8" stroke-linecap="round" />
              <path d={arc(0, 2.5, 4.5)} fill="none" stroke={ink} stroke-width="1.8" stroke-linecap="round" />
              <circle cx="0" cy="2.6" r="1.4" />
              <circle cx="-5.4" cy="8.4" r="1.2" />
              <circle cx="-1.8" cy="9.2" r="1.2" />
              <circle cx="1.8" cy="9.2" r="1.2" />
              <circle cx="5.4" cy="8.4" r="1.2" fill-opacity="0.35" />
            </g>
          </g>
        {:else}
          <!-- Traditional horizontal top status bar: iPhone 16 Pro, Pixel 9, and Duo open portrait. -->
          <g fill={ink}>
            <text
              x={timeX}
              y={barY + (ios ? 6 : 5)}
              text-anchor={p.art === "iphone" ? "middle" : "start"}
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

          {#if p.art === "iphone"}
            <!-- iPhone 16 Pro horizontal Dynamic Island, with the front camera lens at its right. -->
            <rect x={cx - 63} y={sy + 11} width="126" height="37" rx="18.5" fill="#000" />
            <circle cx={cx + 44} cy={sy + 29.5} r="5.5" fill="#0b0d12" />
            <circle cx={cx + 42.8} cy={sy + 28.3} r="1.6" fill="#1f2738" />
          {:else if p.art === "android"}
            <!-- Pixel 9 centred punch-hole camera. -->
            <circle cx={cx} cy={barY} r="10" fill="#000" />
            <circle cx={cx} cy={barY} r="4" fill="#0b0d12" />
            <circle cx={cx - 1.2} cy={barY - 1.2} r="1.2" fill="#232b3c" />
          {/if}
        {/if}

        <!-- Home indicator pill. -->
        <rect
          x={pillCx - pill / 2}
          y={p.art === "android" ? sy + p.height - 14 : sy + p.height - 13}
          width={pill}
          height={p.art === "android" ? 4 : 5}
          rx={p.art === "android" ? 2 : 2.5}
          fill={ink}
          fill-opacity="0.9"
        />
      </g>
    </svg>
  {/if}
  <div
    class="screen"
    class:on={!!p}
    style:left={p ? `${(sx + (p.left ?? 0)) * fit}px` : null}
    style:top={p ? `${(sy + p.top) * fit}px` : null}
  >
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
