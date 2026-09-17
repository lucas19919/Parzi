<script lang="ts">
  // Appearance: every field of theme.toml, in the order you'd reach for it.
  // Edits preview instantly (inline vars), persist ~400ms later, then the
  // authoritative stylesheet from Rust replaces the preview.
  import { onMount, onDestroy } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api, type Theme, type PackInfo, type BackgroundFile } from "../api";
  import { applyThemeCss, previewTheme, refreshBackground, titleCase } from "../theme";
  import Switch from "./Switch.svelte";
  import SegControl from "./SegControl.svelte";
  import Slider from "./Slider.svelte";
  import ColorField from "./ColorField.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  let theme: Theme | null = null;
  let packs: PackInfo[] = [];
  let backgrounds: BackgroundFile[] = [];
  let loading = true;
  let err = "";
  let packName = "";
  let fileInputEl: HTMLInputElement | null = null;
  let sampling = false;
  let scanOn = false;

  // Whole-number twins for the 0..1 fields.
  let dimPct = 66;
  let vignettePct = 50;
  let opacityPct = 85;

  const ACCENTS = ["#7C8CFF", "#7AA2F7", "#88C0D0", "#CBA6F7", "#EB6F92", "#F5A97F", "#A6DA95", "#E0DEF4"];
  const UI_FONTS = ["Inter", "system-ui", "Segoe UI", "SF Pro Text", "Roboto", "IBM Plex Sans"];
  const MONO_FONTS = ["JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "SF Mono", "ui-monospace"];
  const SIZES_UI = [12, 13, 14, 15, 16].map((n) => ({ value: String(n), label: String(n) }));
  const SIZES_MONO = [11, 12, 13, 14, 15].map((n) => ({ value: String(n), label: String(n) }));
  const COLOR_KEYS = ["sidebar", "stage", "bar", "border", "accent", "text", "text_dim"] as const;
  type ColorKey = (typeof COLOR_KEYS)[number];

  onMount(() => {
    scanOn = document.documentElement.classList.contains("scanlines");
    void load();
  });
  onDestroy(() => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      void commit();
    }
  });

  async function load() {
    loading = true;
    err = "";
    try {
      const [t, p, b] = await Promise.all([
        api.getTheme(),
        api.listPackInfos(),
        api.listBackgroundUrls(),
      ]);
      theme = t;
      packs = p;
      backgrounds = b;
      syncPercents();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  function syncPercents() {
    if (!theme) return;
    dimPct = Math.round(theme.background.dim * 100);
    vignettePct = Math.round(theme.background.vignette * 100);
    opacityPct = Math.round(theme.glass.opacity * 100);
  }

  function sameColors(a: Theme["colors"], b: Theme["colors"]): boolean {
    return COLOR_KEYS.every((k) => (a[k] || "").toLowerCase() === (b[k] || "").toLowerCase());
  }
  function findActive(t: Theme | null, ps: PackInfo[]): string {
    if (!t) return "";
    return ps.find((p) => sameColors(p.colors, t.colors))?.name ?? "";
  }
  $: activePack = findActive(theme, packs);

  // ---- edit pipeline -------------------------------------------------------
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function touch() {
    if (!theme) return;
    theme = theme;
    previewTheme(theme);
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(commit, 400);
  }
  async function commit() {
    saveTimer = null;
    if (!theme) return;
    try {
      await api.saveTheme(theme);
      applyThemeCss(await api.getThemeCss());
    } catch (e) {
      notify(`Save failed: ${e}`);
      // Never leave a preview the disk state doesn't match: fall back to
      // the authoritative stylesheet and re-read the saved theme.
      try {
        applyThemeCss(await api.getThemeCss());
        theme = await api.getTheme();
        syncPercents();
      } catch {}
    }
  }
  async function commitNow(msg = "") {
    if (saveTimer) clearTimeout(saveTimer);
    await commit();
    if (msg) notify(msg);
  }

  // ---- themes ---------------------------------------------------------------
  async function applyPack(p: PackInfo) {
    renamingPack = null;
    delPack = null;
    try {
      await commitNow();
      applyThemeCss(await api.applyPack(p.name));
      theme = await api.getTheme();
      // Packs can ship art: re-read the gallery or the new picture has no
      // thumbnail and the wallpaper highlight cannot move to it.
      backgrounds = await api.listBackgroundUrls();
      syncPercents();
      await refreshBackground();
      if (theme.background.auto_accent && theme.background.image) await sampleAccent(true);
      notify(`Theme: ${titleCase(p.name)}`);
    } catch (e) {
      notify(`Could not apply theme: ${e}`);
    }
  }
  function cleanPackName(raw: string): string {
    return raw
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]+/g, "-")
      .replace(/^-+|-+$/g, "");
  }
  async function savePack() {
    const name = cleanPackName(packName);
    if (!name) return;
    try {
      await commitNow();
      await api.savePack(name);
      packs = await api.listPackInfos();
      packName = "";
      notify(`Saved theme: ${titleCase(name)}`);
    } catch (e) {
      notify(`Could not save theme: ${e}`);
    }
  }
  let delPack: string | null = null;
  async function deletePack(p: PackInfo) {
    // Two-step: packs are directories, deletion is forever.
    if (delPack !== p.name) {
      delPack = p.name;
      return;
    }
    delPack = null;
    try {
      await api.deletePack(p.name);
      packs = await api.listPackInfos();
      notify(`Deleted ${titleCase(p.name)}`);
    } catch (e) {
      notify(`Could not delete theme: ${e}`);
    }
  }

  let renamingPack: string | null = null;
  let renameDraft = "";
  function focusRename(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
  async function commitPackRename(p: PackInfo) {
    const name = cleanPackName(renameDraft);
    renamingPack = null;
    if (!name || name === p.name) return;
    try {
      await api.renamePack(p.name, name);
      packs = await api.listPackInfos();
      notify(`Renamed to ${titleCase(name)}`);
    } catch (e) {
      notify(`Could not rename: ${e}`);
    }
  }

  // ---- wallpaper --------------------------------------------------------------
  async function setBg(name: string) {
    delWall = null;
    try {
      // Flush pending edits first: set_background re-reads theme.toml.
      await commitNow();
      applyThemeCss(await api.setBackground(name));
      theme = await api.getTheme();
      syncPercents();
      await refreshBackground();
      if (name && theme.background.auto_accent) await sampleAccent(true);
      else notify(name ? `Wallpaper: ${name}` : "Wallpaper off");
    } catch (e) {
      notify(`Could not set wallpaper: ${e}`);
    }
  }
  function pickFile() {
    fileInputEl?.click();
  }
  /** Two-step wallpaper delete: first click arms, second click fires. The
      live wallpaper falls back to solid instead of a missing file. */
  let delWall: string | null = null;
  async function deleteWall(b: BackgroundFile) {
    if (delWall !== b.name) {
      delWall = b.name;
      return;
    }
    delWall = null;
    try {
      applyThemeCss(await api.deleteBackground(b.name));
      theme = await api.getTheme();
      backgrounds = await api.listBackgroundUrls();
      syncPercents();
      await refreshBackground();
      notify(`Deleted ${b.name}`);
    } catch (e) {
      notify(`Could not delete image: ${e}`);
    }
  }
  async function onFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = async () => {
      try {
        await commitNow();
        notify("Adding image…");
        const saved = await api.saveBackgroundData(file.name, reader.result as string);
        backgrounds = await api.listBackgroundUrls();
        await setBg(saved);
      } catch (err) {
        notify(`Could not add image: ${err}`);
      }
    };
    reader.onerror = () => notify("Could not read that file");
    reader.readAsDataURL(file);
  }
  async function sampleAccent(silent = false) {
    if (!theme?.background.image) {
      if (!silent) notify("Choose a wallpaper first");
      return;
    }
    sampling = true;
    try {
      const pal = await api.paletteFromBackground();
      if (theme) theme.colors.accent = pal.accent;
      await commitNow(silent ? "" : `Accent from wallpaper: ${pal.accent}`);
    } catch (e) {
      if (!silent) notify(`Could not sample the wallpaper: ${e}`);
    } finally {
      sampling = false;
    }
  }
  async function toggleAutoAccent() {
    if (!theme) return;
    theme.background.auto_accent = !theme.background.auto_accent;
    await commitNow();
    if (theme.background.auto_accent) {
      await sampleAccent(true);
      notify("Accent follows the wallpaper");
    } else notify("Accent no longer follows the wallpaper");
  }

  // ---- fields ----------------------------------------------------------------
  function onDim() {
    if (theme) {
      theme.background.dim = dimPct / 100;
      touch();
    }
  }
  function onVignette() {
    if (theme) {
      theme.background.vignette = vignettePct / 100;
      touch();
    }
  }
  function onOpacity() {
    if (theme) {
      theme.glass.opacity = opacityPct / 100;
      touch();
    }
  }
  function setColor(key: ColorKey, hex: string) {
    if (!theme) return;
    theme.colors[key] = hex;
    touch();
  }
  function setSize(key: "size" | "mono_size", v: string) {
    if (!theme) return;
    theme.font[key] = Number(v);
    touch();
  }

  // ---- custom css: hackers only ------------------------------------------------
  // `~/.parzi/user.css` still loads last and wins (backend keeps it,
  // packs snapshot it) — there is just no editor for it here anymore.

  async function resetAppearance() {
    try {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = null;
      await api.resetTheme();
      applyThemeCss(await api.getThemeCss());
      theme = await api.getTheme();
      syncPercents();
      await refreshBackground();
      notify("Appearance reset");
    } catch (e) {
      notify(`Reset failed: ${e}`);
    }
  }
</script>

{#if loading}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="packs"><div class="skel" /><div class="skel" /><div class="skel" /><div class="skel" /></div>
    <div class="skel" />
  </div>
{:else if err || !theme}
  <div class="load-err">
    <span>{err || "Theme failed to load"}</span>
    <button class="sbtn" on:click={load}>Retry</button>
  </div>
{:else}
  <!-- Theme -->
  <section class="pref-section">
    <h3 class="section-title">Theme</h3>
    <p class="section-desc">A theme sets the seven colours and the glass. Your wallpaper stays.</p>
    <div class="packs">
      {#each packs as p (p.name)}
        {@const c = p.colors}
        <div class="pack" class:on={activePack === p.name}>
          <button
            class="pack-btn"
            on:click={() => applyPack(p)}
            title={p.builtin ? titleCase(p.name) : `${titleCase(p.name)} (yours)`}
            aria-pressed={activePack === p.name}
          >
            <span
              class="pp"
              style="--pp-sb:{c.sidebar};--pp-st:{c.stage};--pp-bar:{c.bar};--pp-bd:{c.border};--pp-ac:{c.accent};--pp-tx:{c.text};--pp-td:{c.text_dim}"
            >
              <span class="pp-side"><i /><i /><i /></span>
              <span class="pp-stage">
                <span class="pp-line" /><span class="pp-line short" />
                <span class="pp-bar"><i /></span>
              </span>
            </span>
            <span class="pack-name">{#if renamingPack === p.name && !p.builtin}<input class="pack-rename" bind:value={renameDraft} use:focusRename on:click|stopPropagation on:keydown={(e) => { if (e.key === "Enter") commitPackRename(p); else if (e.key === "Escape") renamingPack = null; }} />{:else}{titleCase(p.name)}{/if}{#if p.has_art} <span class="art-dot" title="Switches the wallpaper too">🖼</span>{/if}</span>
          </button>
          {#if !p.builtin}
            <div class="pack-actions">
              <button class="pack-act" title="Rename {titleCase(p.name)}" aria-label="Rename {titleCase(p.name)}"
                on:click={() => { renamingPack = p.name; renameDraft = p.name; delPack = null; }}>✎</button>
              <button class="pack-del" class:armed={delPack === p.name}
                title={delPack === p.name ? "Click again to delete this theme" : "Delete this theme"}
                aria-label="Delete {titleCase(p.name)}" on:click={() => deletePack(p)}>{delPack === p.name ? "sure?" : "×"}</button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="save-row">
      <input
        class="text"
        placeholder="Save the current look as…"
        bind:value={packName}
        on:keydown={(e) => e.key === "Enter" && savePack()}
        spellcheck="false"
      />
      <button class="sbtn" disabled={!packName.trim()} on:click={savePack}>Save theme</button>
    </div>
  </section>

  <!-- Wallpaper -->
  <section class="pref-section">
    <div class="section-head-with-action">
      <div>
        <h3 class="section-title">Wallpaper</h3>
        <p class="section-desc">Sits behind everything. Dim it until text reads comfortably.</p>
      </div>
      <button class="sbtn" on:click={pickFile}>Add image…</button>
      <input type="file" accept="image/png,image/jpeg,image/webp" hidden bind:this={fileInputEl} on:change={onFile} />
    </div>
    <div class="walls">
      <button class="wall" class:on={!theme.background.image} on:click={() => setBg("")}>
        <span class="wall-pre none"><span>Off</span></span>
        <span class="wall-name">None</span>
      </button>
      {#each backgrounds as b (b.name)}
        <div class="wall-wrap">
          <button
            class="wall"
            class:on={theme.background.image.endsWith("/" + b.name)}
            on:click={() => setBg(b.name)}
            title={b.name}
          >
            <img class="wall-pre" src={convertFileSrc(b.url)} alt="" loading="lazy" draggable="false" />
            <span class="wall-name">{b.name}</span>
          </button>
          <button class="wall-del" class:armed={delWall === b.name}
            title={delWall === b.name ? "Click again to delete this image" : `Delete ${b.name}`}
            aria-label={delWall === b.name ? `Confirm delete ${b.name}` : `Delete ${b.name}`}
            on:click={() => deleteWall(b)}>{delWall === b.name ? "sure?" : "×"}</button>
        </div>
      {/each}
    </div>
    <div class="field-card stack">
      <Slider label="Dim" bind:value={dimPct} min={0} max={95} unit="%" on:input={onDim} />
      <Slider label="Blur" bind:value={theme.background.blur} min={0} max={20} step={0.5} unit="px" on:input={touch} />
      <Slider label="Vignette" bind:value={vignettePct} min={0} max={90} unit="%" on:input={onVignette} />
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Accent follows the wallpaper</span>
        <span class="field-hint">Samples the most vivid colour whenever the picture changes.</span>
      </div>
      <div class="inline-actions">
        <button class="sbtn" disabled={sampling || !theme.background.image} on:click={() => sampleAccent()}>
          {sampling ? "Sampling…" : "Sample now"}
        </button>
        <Switch on={theme.background.auto_accent ?? false} title="Accent follows the wallpaper" on:toggle={toggleAutoAccent} />
      </div>
    </div>
  </section>

  <!-- Accent -->
  <section class="pref-section">
    <h3 class="section-title">Accent</h3>
    <p class="section-desc">The one colour with a job: selection, focus, the send button.</p>
    <div class="field-card">
      <div class="swatches">
        {#each ACCENTS as hex}
          <button
            class="swatch"
            class:on={theme.colors.accent.toLowerCase() === hex.toLowerCase()}
            style="--sw:{hex}"
            title={hex}
            aria-label="Accent {hex}"
            on:click={() => setColor("accent", hex)}
          />
        {/each}
        <span class="swatch-gap" />
        <ColorField label="Custom" value={theme.colors.accent} on:input={(e) => setColor("accent", e.detail)} />
      </div>
    </div>
  </section>

  <!-- Colours -->
  <section class="pref-section">
    <h3 class="section-title">Colours</h3>
    <p class="section-desc">Everything else derives from these six. Edit them, or leave them to the theme.</p>
    <div class="field-card grid2">
      <ColorField label="Sidebar" value={theme.colors.sidebar} on:input={(e) => setColor("sidebar", e.detail)} />
      <ColorField label="Stage" value={theme.colors.stage} on:input={(e) => setColor("stage", e.detail)} />
      <ColorField label="Composer" value={theme.colors.bar} on:input={(e) => setColor("bar", e.detail)} />
      <ColorField label="Border" value={theme.colors.border} on:input={(e) => setColor("border", e.detail)} />
      <ColorField label="Text" value={theme.colors.text} on:input={(e) => setColor("text", e.detail)} />
      <ColorField label="Muted text" value={theme.colors.text_dim} on:input={(e) => setColor("text_dim", e.detail)} />
    </div>
  </section>

  <!-- Type -->
  <section class="pref-section">
    <h3 class="section-title">Type</h3>
    <p class="section-desc">Any installed font. Inter and JetBrains Mono ship with Parzi.</p>
    <div class="field-card grid2">
      <label class="fld">
        <span>Interface font</span>
        <input class="text" list="parzi-ui-fonts" bind:value={theme.font.family} on:input={touch} spellcheck="false" />
      </label>
      <div class="fld">
        <span>Size</span>
        <SegControl options={SIZES_UI} value={String(theme.font.size)} on:pick={(e) => setSize("size", e.detail)} />
      </div>
      <label class="fld">
        <span>Code font</span>
        <input class="text" list="parzi-mono-fonts" bind:value={theme.font.mono} on:input={touch} spellcheck="false" />
      </label>
      <div class="fld">
        <span>Code size</span>
        <SegControl options={SIZES_MONO} value={String(theme.font.mono_size)} on:pick={(e) => setSize("mono_size", e.detail)} />
      </div>
    </div>
    <datalist id="parzi-ui-fonts">{#each UI_FONTS as f}<option value={f} />{/each}</datalist>
    <datalist id="parzi-mono-fonts">{#each MONO_FONTS as f}<option value={f} />{/each}</datalist>
  </section>

  <!-- Glass -->
  <section class="pref-section">
    <h3 class="section-title">Glass</h3>
    <p class="section-desc">The composer, menus and the command palette.</p>
    <div class="field-card stack">
      <Slider label="Opacity" bind:value={opacityPct} min={40} max={100} unit="%" on:input={onOpacity} />
      <Slider label="Blur" bind:value={theme.glass.blur_px} min={0} max={40} unit="px" on:input={touch} />
      <Slider label="Corner radius" bind:value={theme.glass.radius} min={0} max={24} unit="px" on:input={touch} />
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Scanlines</span>
        <span class="field-hint">A faint raster over the window. Off by default.</span>
      </div>
      <Switch
        on={scanOn}
        title="Scanlines"
        on:toggle={() => {
          scanOn = document.documentElement.classList.toggle("scanlines");
          try { localStorage.setItem("parzi.scanlines", scanOn ? "1" : "0"); } catch {}
        }}
      />
    </div>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Shadows</span>
        <span class="field-hint">Depth under floating surfaces.</span>
      </div>
      <Switch
        on={theme.glass.shadow}
        title="Shadows"
        on:toggle={() => {
          if (theme) {
            theme.glass.shadow = !theme.glass.shadow;
            touch();
          }
        }}
      />
    </div>
  </section>

  <!-- Hackers: `~/.parzi/user.css` still loads last and wins — there is
       just no editor for it here anymore. -->

  <div class="actions-row-end">
    <button class="sbtn" on:click={resetAppearance}>Reset appearance</button>
  </div>
{/if}

<style>
  /* Theme cards: a real thumbnail of the palette, not four swatches. */
  .packs { display: grid; grid-template-columns: repeat(auto-fill, minmax(148px, 1fr)); gap: 10px; }
  .pack { position: relative; }
  .pack-btn {
    width: 100%; display: flex; flex-direction: column; gap: 7px; padding: 5px;
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-3);
    cursor: pointer; text-align: left; font: inherit; color: var(--text-2);
  }
  .pack-btn:hover { background: var(--surface-1); border-color: var(--line-2); color: var(--text); }
  .pack.on .pack-btn { border-color: var(--accent-line); background: var(--accent-soft); color: var(--text); }
  .pp {
    display: grid; grid-template-columns: 30% 1fr; height: 72px;
    border-radius: var(--radius-2); overflow: hidden; border: 1px solid var(--pp-bd);
  }
  .pp-side { background: var(--pp-sb); padding: 9px 7px; display: flex; flex-direction: column; gap: 5px; }
  .pp-side i { display: block; height: 4px; border-radius: 2px; background: var(--pp-td); opacity: 0.7; width: 70%; }
  .pp-side i:first-child { background: var(--pp-tx); width: 48%; }
  .pp-stage { background: var(--pp-st); position: relative; padding: 11px 10px 0; display: flex; flex-direction: column; gap: 5px; }
  .pp-line { display: block; height: 4px; border-radius: 2px; background: var(--pp-tx); opacity: 0.85; width: 62%; }
  .pp-line.short { width: 40%; background: var(--pp-td); }
  .pp-bar {
    position: absolute; left: 10px; right: 10px; bottom: 8px; height: 16px;
    border-radius: 5px; background: var(--pp-bar); border: 1px solid var(--pp-bd);
  }
  .pp-bar i { position: absolute; right: 4px; top: 3px; width: 8px; height: 8px; border-radius: 3px; background: var(--pp-ac); }
  .pack-name { font-size: 12px; padding: 0 3px; }
  .art-dot { font-size: 11px; opacity: 0.75; }
  .pack-actions {
    position: absolute; top: 6px; right: 6px; display: flex; gap: 2px; opacity: 0;
  }
  .pack:hover .pack-actions, .pack-actions:focus-within { opacity: 1; }
  .pack-actions .pack-del { position: static; opacity: 1; }
  .pack-act {
    width: 20px; height: 20px; line-height: 1;
    border-radius: var(--radius-1); border: none; background: var(--menu); color: var(--text-3);
    font: inherit; font-size: 11px; cursor: pointer; padding: 0;
    display: inline-flex; align-items: center; justify-content: center;
  }
  .pack-act:hover { color: var(--text); }
  .pack-rename {
    font: inherit; font-size: 12px; color: var(--text);
    background: var(--input); border: 1px solid var(--accent-line); border-radius: 5px;
    padding: 1px 5px; width: 100%; box-sizing: border-box; outline: none;
  }
  .pack-del {
    position: absolute; top: 6px; right: 6px; width: 20px; height: 20px; line-height: 1;
    border-radius: var(--radius-1); border: none; background: var(--menu); color: var(--text-3);
    font: inherit; font-size: 14px; cursor: pointer; opacity: 0;
  }
  .pack:hover .pack-del, .pack-del:focus-visible { opacity: 1; }
  .pack-del:hover { color: var(--bad); }

  .save-row { display: flex; gap: 8px; }
  .text {
    flex: 1; min-width: 0; background: var(--input); border: 1px solid var(--line-2);
    border-radius: var(--radius-2); color: var(--text); padding: 7px 10px;
    font: inherit; font-size: 12.5px; outline: none;
  }
  .text::placeholder { color: var(--text-4); }
  .text:focus { border-color: var(--accent-line) !important; box-shadow: none; }

  /* Wallpaper gallery. */
  .walls { display: grid; grid-template-columns: repeat(auto-fill, minmax(124px, 1fr)); gap: 10px; }
  .wall {
    display: flex; flex-direction: column; gap: 6px; padding: 5px;
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-3);
    cursor: pointer; font: inherit; color: var(--text-3); text-align: left;
  }
  .wall:hover { background: var(--surface-1); border-color: var(--line-2); color: var(--text); }
  .wall.on { border-color: var(--accent-line); background: var(--accent-soft); color: var(--text); }
  .wall-pre {
    display: block; width: 100%; aspect-ratio: 16 / 10; object-fit: cover;
    border-radius: var(--radius-2); border: 1px solid var(--line-2);
  }
  .wall-pre.none {
    background: var(--stage); display: flex; align-items: center; justify-content: center;
    font-size: 11px; color: var(--text-4);
  }
  .wall-name { font-size: 11.5px; padding: 0 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wall-wrap { position: relative; min-width: 0; }
  .wall-wrap .wall { width: 100%; }
  .wall-del {
    position: absolute; top: 8px; right: 8px; min-width: 20px; height: 20px; line-height: 1;
    border-radius: var(--radius-1); border: none; background: var(--menu); color: var(--text-3);
    font: inherit; font-size: 12px; cursor: pointer; opacity: 0; padding: 0 3px;
  }
  .wall-wrap:hover .wall-del, .wall-del:focus-visible { opacity: 1; }
  .wall-del:hover { color: var(--bad); }
  .wall-del.armed { opacity: 1; color: var(--bad); font-size: 10px; font-weight: 600; padding: 0 7px; }

  .field-card.stack { flex-direction: column; align-items: stretch; gap: 12px; }
  /* Auto-fit (not a viewport breakpoint): columns stack based on the card's
     real width, so narrow stages never squeeze fields into each other. */
  .field-card.grid2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 12px 24px; align-items: center; }
  .inline-actions { display: flex; align-items: center; gap: 12px; flex: none; }

  .swatches { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; width: 100%; }
  .swatch-gap { flex: 1; }
  .swatch {
    width: 24px; height: 24px; border-radius: 50%; background: var(--sw);
    border: 2px solid transparent; padding: 0; cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
    transition: transform var(--dur-lift) var(--ease-spring), border-color var(--dur-lift) ease;
  }
  .swatch:hover { transform: scale(1.1); }
  .swatch.on { border-color: var(--text); }

  .fld { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 12.5px; color: var(--text-2); min-width: 0; }
  .fld > span { flex: none; }
  .fld .text { max-width: 190px; }

  .actions-row-end { display: flex; justify-content: flex-end; }
</style>
