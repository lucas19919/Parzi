<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type Theme, type PackInfo, type BackgroundFile } from "../api";
  import { applyThemeCss, previewTheme, refreshBackground } from "../theme";
  import Icon from "../Icon.svelte";
  import SegControl from "./SegControl.svelte";
  import Slider from "./Slider.svelte";
  import ColorField from "./ColorField.svelte";
  import { tooltip } from "../tooltip";
  import "./shared.css";

  export let notify: (msg: string, err?: boolean) => void = () => {};

  let theme: Theme | null = null;
  let packs: PackInfo[] = [];
  let backgrounds: BackgroundFile[] = [];
  let yPct = 50;
  let loading = true;
  let err = "";
  let packName = "";
  let fileInputEl: HTMLInputElement | null = null;



  const ACCENTS = ["#E8E8E8", "#CBD5E1", "#7AA2F7", "#88C0D0", "#34D399", "#A6DA95", "#5EEAD4", "#EB6F92", "#F5A97F", "#E8B64C"];
  const UI_FONTS = ["Inter", "system-ui", "Segoe UI", "SF Pro Text", "Roboto", "IBM Plex Sans"];
  const MONO_FONTS = ["JetBrains Mono", "Cascadia Code", "Fira Code", "Consolas", "SF Mono", "ui-monospace"];
  const SIZES_UI = [12, 13, 14, 15, 16].map((n) => ({ value: String(n), label: String(n) }));
  const SIZES_MONO = [11, 12, 13, 14, 15].map((n) => ({ value: String(n), label: String(n) }));
  const COLOR_KEYS = ["stage", "accent", "text"] as const;
  type ColorKey = (typeof COLOR_KEYS)[number] | "panel";

  onMount(() => {
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
      syncPos();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  function onPos(v: number) {
    if (!theme || !Number.isFinite(v)) return;
    yPct = Math.round(v);
    theme.background.y = yPct / 100;
    touch();
  }

  function syncPos() {
    const y = theme?.background.y;
    yPct = typeof y === "number" && Number.isFinite(y) ? Math.round(y * 100) : 50;
  }

  function sameColors(a: Theme["colors"], b: Theme["colors"]): boolean {
    return COLOR_KEYS.filter((k) => k !== "accent").every((k) => (a[k] || "").toLowerCase() === (b[k] || "").toLowerCase());
  }
  function findActive(t: Theme | null, ps: PackInfo[]): string {
    if (!t) return "";
    return ps.find((p) => sameColors(p.colors, t.colors))?.name ?? "";
  }
  $: activePack = findActive(theme, packs);

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
      notify(`Could not save: ${e}`, true);
      try {
        applyThemeCss(await api.getThemeCss());
        theme = await api.getTheme();
      } catch {}
    }
  }
  async function commitNow() {
    if (saveTimer) clearTimeout(saveTimer);
    await commit();
  }

  async function applyPack(p: PackInfo) {
    renamingPack = null;
    delPack = null;
    try {
      await commitNow();
      applyThemeCss(await api.applyPack(p.name));
      theme = await api.getTheme();
      syncPos();
      const [b] = await Promise.all([api.listBackgroundUrls(), refreshBackground()]);
      backgrounds = b;
    } catch (e) {
      notify(`Could not apply the theme: ${e}`, true);
    }
  }
  function packLabel(slug: string): string {
    const words = slug.replace(/[-_]+/g, " ").trim();
    return words ? words[0].toUpperCase() + words.slice(1) : slug;
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
    } catch (e) {
      notify(`Could not save the theme: ${e}`, true);
    }
  }
  let delPack: string | null = null;
  async function deletePack(p: PackInfo) {
    if (delPack !== p.name) {
      delPack = p.name;
      return;
    }
    delPack = null;
    try {
      await api.deletePack(p.name);
      packs = await api.listPackInfos();
    } catch (e) {
      notify(`Could not delete the theme: ${e}`, true);
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
    } catch (e) {
      notify(`Could not rename: ${e}`, true);
    }
  }

  async function setBg(name: string) {
    delWall = null;
    try {
      await commitNow();
      applyThemeCss(await api.setBackground(name));
      theme = await api.getTheme();
      syncPos();
      await refreshBackground();
    } catch (e) {
      notify(`Could not set the backdrop: ${e}`, true);
    }
  }
  function pickFile() {
    fileInputEl?.click();
  }
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
      syncPos();
      backgrounds = await api.listBackgroundUrls();
      await refreshBackground();
    } catch (e) {
      notify(`Could not delete the image: ${e}`, true);
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
        const saved = await api.saveBackgroundData(file.name, reader.result as string);
        backgrounds = await api.listBackgroundUrls();
        await setBg(saved);
      } catch (err) {
        notify(`Could not add the image: ${err}`, true);
      }
    };
    reader.onerror = () => notify("Could not read that file", true);
    reader.readAsDataURL(file);
  }
  function setColor(key: ColorKey, hex: string) {
    if (!theme) return;
    theme.colors[key] = hex;
    touch();
  }
  function clearPanel() {
    if (!theme) return;
    theme.colors.panel = "";
    touch();
  }
  function setSize(key: "size" | "mono_size", v: string) {
    if (!theme) return;
    theme.font[key] = Number(v);
    touch();
  }

  async function resetAppearance() {
    if (!(await ask("Theme, backdrop, colours and fonts go back to the defaults.", { title: "Reset appearance?", kind: "warning" }))) return;
    try {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = null;
      await api.resetTheme();
      applyThemeCss(await api.getThemeCss());
      theme = await api.getTheme();
      syncPos();
      await refreshBackground();
    } catch (e) {
      notify(`Could not reset: ${e}`, true);
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
  <section class="pref-section">
    <h3 class="section-title">Theme</h3>
    <div class="packs">
      {#each packs as p (p.name)}
        {@const c = p.colors}
        <div class="pack" class:on={activePack === p.name}>
          <button
            class="pack-btn"
            on:click={() => applyPack(p)}
            title={p.builtin ? packLabel(p.name) : `${packLabel(p.name)} (yours)`}
            aria-pressed={activePack === p.name}
          >
            <span
              class="pp"
              style="--pp-st:{c.stage};--pp-ac:{c.accent};--pp-tx:{c.text};--pp-td:{c.text_dim}"
            >
              <span class="pp-line" /><span class="pp-line short" />
              <span class="pp-bar"><i /></span>
            </span>
            <span class="pack-name">{#if renamingPack === p.name && !p.builtin}<input class="pack-rename" aria-label="New name for {packLabel(p.name)}" bind:value={renameDraft} use:focusRename on:click|stopPropagation on:keydown={(e) => { if (e.key === "Enter") commitPackRename(p); else if (e.key === "Escape") renamingPack = null; }} />{:else}{packLabel(p.name)}{/if}</span>{#if p.has_art}<span class="art-dot" use:tooltip={"Comes with a wallpaper"}><Icon name="image" size={12} /></span>{/if}
          </button>
          {#if !p.builtin}
            <div class="pack-actions">
              <button class="mini-icon" use:tooltip={`Rename ${packLabel(p.name)}`}
                on:click={() => { renamingPack = p.name; renameDraft = p.name; delPack = null; }}><Icon name="pencil" size={12} /></button>
              <button class="mini-icon" class:armed={delPack === p.name}
                use:tooltip={delPack === p.name ? "Click again to delete" : "Delete"}
                aria-label="Delete {packLabel(p.name)}" on:click={() => deletePack(p)}>{#if delPack === p.name}Sure?{:else}<Icon name="trash" size={12} />{/if}</button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="save-row">
      <input
        class="tin"
        placeholder="Save the current look as…"
        aria-label="Theme name"
        bind:value={packName}
        on:keydown={(e) => e.key === "Enter" && savePack()}
        spellcheck="false"
      />
      <button class="sbtn" disabled={!packName.trim()} on:click={savePack}>Save</button>
    </div>
  </section>

  <section class="pref-section">
    <div class="section-head-with-action">
      <h3 class="section-title">Backdrop</h3>
      <button class="sbtn" on:click={pickFile}><Icon name="plus" size={12} /><span>Add image</span></button>
      <input type="file" accept="image/png,image/jpeg,image/webp" hidden bind:this={fileInputEl} on:change={onFile} />
    </div>
    <p class="section-desc">One picture behind a new session. Nothing else.</p>
    {#if theme.background.image}
      <div class="field-card stack">
        <Slider label="Position" bind:value={yPct} min={0} max={100} unit="%" on:input={(e) => onPos(e.detail)} />
      </div>
    {/if}
    <div class="walls">
      <button class="wall" class:on={!theme.background.image} on:click={() => setBg("")}>
        <span class="wall-pre none"><span>None</span></span>
      </button>
      {#each backgrounds as b (b.name)}
        <div class="wall-wrap">
          <button
            class="wall"
            class:on={theme.background.image.endsWith("/" + b.name)}
            on:click={() => setBg(b.name)}
            title={b.name}
          >
            <img class="wall-pre" src={convertFileSrc(b.url)} alt={b.name} loading="lazy" draggable="false" />
          </button>
          <button class="mini-icon" class:armed={delWall === b.name}
            use:tooltip={delWall === b.name ? "Click again to delete" : "Delete"}
            aria-label={delWall === b.name ? `Confirm delete ${b.name}` : `Delete ${b.name}`}
            on:click={() => deleteWall(b)}>{#if delWall === b.name}Sure?{:else}<Icon name="close" size={12} />{/if}</button>
        </div>
      {/each}
    </div>
  </section>

  <section class="pref-section">
    <h3 class="section-title">Colours</h3>
    <div class="rows">
      <div class="row">
        <div class="swatches">
          {#each ACCENTS as hex}
            <button
              class="swatch"
              class:on={theme.colors.accent.toLowerCase() === hex.toLowerCase()}
              style="--sw:{hex}"
              use:tooltip={hex}
              aria-label="Accent {hex}"
              on:click={() => setColor("accent", hex)}
            />
          {/each}
          <span class="swatch-gap" />
          <ColorField label="Accent" value={theme.colors.accent} on:input={(e) => setColor("accent", e.detail)} />
        </div>
      </div>
      <div class="row"><ColorField label="Background" value={theme.colors.stage} on:input={(e) => setColor("stage", e.detail)} /></div>
      <div class="row">
        <span class="panel-wrap" class:auto={!theme.colors.panel}>
          <ColorField label="Panels" value={theme.colors.panel || "#808080"} on:input={(e) => setColor("panel", e.detail)} />
        </span>
        {#if theme.colors.panel}
          <button class="mini" use:tooltip={"Back to automatic"} on:click={clearPanel}>Auto</button>
        {/if}
      </div>
      <div class="row"><ColorField label="Text" value={theme.colors.text} on:input={(e) => setColor("text", e.detail)} /></div>
    </div>
  </section>

  <section class="pref-section">
    <h3 class="section-title">Type</h3>
    <div class="rows">
      <div class="row">
        <span class="row-label">Interface font</span>
        <input class="tin fixed" list="parzi-ui-fonts" bind:value={theme.font.family} on:input={touch} spellcheck="false" aria-label="Interface font" />
      </div>
      <div class="row">
        <span class="row-label">Size</span>
        <SegControl label="Interface font size" options={SIZES_UI} value={String(theme.font.size)} on:pick={(e) => setSize("size", e.detail)} />
      </div>
      <div class="row">
        <span class="row-label">Code font</span>
        <input class="tin fixed mono" list="parzi-mono-fonts" bind:value={theme.font.mono} on:input={touch} spellcheck="false" aria-label="Code font" />
      </div>
      <div class="row">
        <span class="row-label">Code size</span>
        <SegControl label="Code font size" options={SIZES_MONO} value={String(theme.font.mono_size)} on:pick={(e) => setSize("mono_size", e.detail)} />
      </div>
    </div>
    <datalist id="parzi-ui-fonts">{#each UI_FONTS as f}<option value={f} />{/each}</datalist>
    <datalist id="parzi-mono-fonts">{#each MONO_FONTS as f}<option value={f} />{/each}</datalist>
  </section>

  <div class="actions-row-end">
    <button class="sbtn" on:click={resetAppearance}>Reset appearance</button>
  </div>
{/if}

<style>
  .packs { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: calc(var(--space) * 3); }
  .pack { position: relative; }
  .pack-btn {
    width: 100%; display: flex; flex-direction: column; gap: calc(var(--space) * 2); padding: var(--space);
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-lg);
    cursor: pointer; text-align: left; font: inherit; color: var(--muted);
  }
  .pack-btn:hover { background: var(--panel); border-color: var(--line); color: var(--text); }
  .pack.on .pack-btn { border-color: var(--accent); background: var(--line); color: var(--text); }
  .pp {
    position: relative; display: flex; flex-direction: column; gap: var(--space); height: 68px; padding: calc(var(--space) * 3) calc(var(--space) * 3) 0;
    border-radius: var(--radius); overflow: hidden; background: var(--pp-st);
    border: 1px solid color-mix(in srgb, var(--pp-tx) 12%, var(--pp-st));
  }
  .pp-line { display: block; height: 4px; border-radius: var(--radius-sm); background: var(--pp-tx); opacity: 0.85; width: 62%; }
  .pp-line.short { width: 40%; background: var(--pp-td); }
  .pp-bar {
    position: absolute; left: 10px; right: 10px; bottom: 8px; height: 16px;
    border-radius: var(--radius-sm); background: color-mix(in srgb, var(--pp-tx) 6%, var(--pp-st));
    border: 1px solid color-mix(in srgb, var(--pp-tx) 12%, var(--pp-st));
  }
  .pp-bar i { position: absolute; right: 4px; top: 3px; width: 8px; height: 8px; border-radius: var(--radius-sm); background: var(--pp-ac); }
  .pack-name { font-size: var(--fs-sm); padding: 0 var(--space); }
  .art-dot { display: inline-flex; flex: none; color: var(--muted); }
  .pack-actions { position: absolute; top: 9px; right: 9px; display: flex; gap: var(--space); opacity: 0; }
  .pack:hover .pack-actions, .pack-actions:focus-within { opacity: 1; }
  .mini-icon {
    min-width: 20px; height: 20px; padding: 0 var(--space); line-height: 1;
    border-radius: var(--radius-sm); border: none; background: var(--panel); color: var(--muted);
    font: inherit; font-size: var(--fs-xs); cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
  }
  .mini-icon:hover { color: var(--text); }
  .mini-icon.armed { color: var(--bad); font-weight: 600; }
  .pack-rename {
    font: inherit; font-size: var(--fs-sm); color: var(--text);
    background: var(--bg); border: 1px solid var(--accent); border-radius: var(--radius-sm);
    padding: var(--space) var(--space); width: 100%; box-sizing: border-box; outline: none;
  }

  .save-row { display: flex; gap: calc(var(--space) * 2); }
  .save-row .tin { flex: 1; }

  .section-head-with-action { align-items: center; }
  .field-card.stack { flex-direction: column; align-items: stretch; gap: calc(var(--space) * 3); }

  .walls { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: calc(var(--space) * 3); }
  .wall {
    display: block; width: 100%; padding: var(--space);
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-lg);
    cursor: pointer; font: inherit; color: var(--muted);
  }
  .wall:hover { background: var(--panel); border-color: var(--line); }
  .wall.on { border-color: var(--accent); background: var(--line); }
  .wall-pre {
    display: block; width: 100%; aspect-ratio: 16 / 10; object-fit: cover;
    border-radius: var(--radius); border: 1px solid var(--line);
  }
  .wall-pre.none {
    background: var(--bg); display: flex; align-items: center; justify-content: center;
    font-size: var(--fs-sm); color: var(--muted);
  }
  .wall-wrap { position: relative; min-width: 0; }
  .wall-wrap .mini-icon { position: absolute; top: 9px; right: 9px; opacity: 0; }
  .wall-wrap:hover .mini-icon, .mini-icon:focus-visible { opacity: 1; }
  .mini-icon.armed { opacity: 1; }

  .rows > .row > :global(.cf) { flex: 1; min-width: 0; }
  .panel-wrap { display: flex; flex: 1; min-width: 0; }
  .panel-wrap.auto { opacity: 0.55; }
  .mini {
    flex: none;
    padding: var(--space) calc(var(--space) * 3);
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .mini:hover {
    color: var(--text);
    background: var(--hover);
  }
  .swatches { display: flex; align-items: center; gap: calc(var(--space) * 3); flex-wrap: wrap; width: 100%; }
  .swatch-gap { flex: 1; }
  .swatch {
    width: 22px; height: 22px; border-radius: 50%; background: var(--sw);
    border: 2px solid transparent; padding: 0; cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25), 0 0 0 1px var(--line);
    transition: transform 140ms ease, border-color 140ms ease;
  }
  .swatch:hover { transform: scale(1.1); }
  .swatch.on { border-color: var(--text); }

  .actions-row-end { display: flex; justify-content: flex-end; }
</style>
