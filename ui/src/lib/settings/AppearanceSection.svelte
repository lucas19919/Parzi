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
  let hPct = 34;
  let loading = true;
  let err = "";
  let packName = "";
  let fileInputEl: HTMLInputElement | null = null;



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

  function onHeight(v: number) {
    if (!theme || !Number.isFinite(v)) return;
    hPct = Math.round(v);
    theme.background.height = hPct / 100;
    touch();
  }

  function syncPos() {
    const y = theme?.background.y;
    yPct = typeof y === "number" && Number.isFinite(y) ? Math.round(y * 100) : 50;
    const h = theme?.background.height;
    hPct = typeof h === "number" && Number.isFinite(h) ? Math.round(h * 100) : 34;
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
      notify(`Could not set the wallpaper: ${e}`, true);
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
    if (!(await ask("Theme, wallpaper, colours and fonts go back to the defaults.", { title: "Reset appearance?", kind: "warning" }))) return;
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
    <button class="btn" on:click={load}>Retry</button>
  </div>
{:else}
  <section class="pref-section">
    <div class="rows pad-even">
      <div class="row row-title"><span>Theme</span></div>
      <div class="row">
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
                <span class="pack-head"><span class="pack-name">{#if renamingPack === p.name && !p.builtin}<input class="pack-rename" aria-label="New name for {packLabel(p.name)}" bind:value={renameDraft} use:focusRename on:click|stopPropagation on:keydown={(e) => { if (e.key === "Enter") commitPackRename(p); else if (e.key === "Escape") renamingPack = null; }} />{:else}{packLabel(p.name)}{/if}</span>{#if p.has_art}<span class="art-dot" use:tooltip={"Comes with a wallpaper"}><Icon name="image" size={12} /></span>{/if}</span>
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
      </div>
      <div class="row save-row">
        <input
          class="tin"
          placeholder="Save the current look as…"
          aria-label="Theme name"
          bind:value={packName}
          on:keydown={(e) => e.key === "Enter" && savePack()}
          spellcheck="false"
        />
        <button class="btn" disabled={!packName.trim()} on:click={savePack}>Save</button>
      </div>
    </div>
  </section>

  <section class="pref-section">
    <input type="file" accept="image/png,image/jpeg,image/webp" hidden bind:this={fileInputEl} on:change={onFile} />
    <div class="rows pad-even">
      <div class="row row-title"><span>Wallpaper</span></div>
      <div class="row thumbs-row">
        <div class="thumbs">
          <button class="thumb" class:on={!theme.background.image} on:click={() => setBg("")} title="No picture" aria-pressed={!theme.background.image}>
            <span class="none">None</span>
          </button>
          {#each backgrounds as b (b.name)}
            <div class="thumb-wrap">
              <button
                class="thumb"
                class:on={theme.background.image.endsWith("/" + b.name)}
                on:click={() => setBg(b.name)}
                title={b.name}
                aria-pressed={theme.background.image.endsWith("/" + b.name)}
              >
                <img src={convertFileSrc(b.url)} alt="" loading="lazy" draggable="false" />
              </button>
              <button class="mini-icon" class:armed={delWall === b.name}
                use:tooltip={delWall === b.name ? "Click again to delete" : `Delete ${b.name}`}
                aria-label={delWall === b.name ? `Confirm delete ${b.name}` : `Delete ${b.name}`}
                on:click={() => deleteWall(b)}>{#if delWall === b.name}Sure?{:else}<Icon name="close" size={12} />{/if}</button>
            </div>
          {/each}
          <button class="thumb add" on:click={pickFile} use:tooltip={"Add image"} aria-label="Add image"><Icon name="plus" size={12} /></button>
        </div>
      </div>
      {#if theme.background.image}
        <div class="row">
          <span class="row-label">Height</span>
          <div class="pos"><Slider bare label="Wallpaper height" bind:value={hPct} min={20} max={60} unit="%" on:input={(e) => onHeight(e.detail)} /></div>
        </div>
        <div class="row">
          <span class="row-label">Position</span>
          <div class="pos"><Slider bare label="Wallpaper position" bind:value={yPct} min={0} max={100} unit="%" on:input={(e) => onPos(e.detail)} /></div>
        </div>
      {/if}
    </div>
  </section>

  <section class="pref-section">
    <div class="rows">
      <div class="row row-title"><span>Colours</span></div>
      <div class="row"><ColorField label="Accent" value={theme.colors.accent} on:input={(e) => setColor("accent", e.detail)} /></div>
      <div class="row"><ColorField label="Background" value={theme.colors.stage} on:input={(e) => setColor("stage", e.detail)} /></div>
      <div class="row">
        <span class="panel-wrap">
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
    <div class="rows">
      <div class="row row-title"><span>Type</span></div>
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
    <button class="btn" on:click={resetAppearance}>Reset appearance</button>
  </div>
{/if}

<style>
  .packs { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: calc(var(--space) * 3); }
  .pack { position: relative; }
  .pack > .pack-btn { width: 100%; }
  .pack.on .pack-btn { border-color: var(--accent); background: var(--line); color: var(--text); }
  .pp {
    position: relative; display: flex; flex-direction: column; gap: var(--space); height: 68px; padding: calc(var(--space) * 3) calc(var(--space) * 3) 0;
    width: 100%; box-sizing: border-box;
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
  .mini-icon.armed { color: var(--bad-text); font-weight: 600; }
  .pack-rename {
    font: inherit; font-size: var(--fs-sm); color: var(--text);
    background: var(--bg); border: 1px solid var(--accent); border-radius: var(--radius-sm);
    padding: var(--space) var(--space); width: 100%; box-sizing: border-box; outline: none;
  }

  .thumbs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: calc(var(--space) * 3);
    width: 100%;
  }
  .thumb {
    width: 100%;
    height: 68px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
    overflow: hidden;
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .thumb:hover { border-color: var(--text); color: var(--text); }
  .thumb.on { border-color: var(--accent); }
  .thumb.add { border-style: dashed; }
  .thumb-wrap { position: relative; }
  .thumb-wrap .mini-icon { position: absolute; top: 4px; right: 4px; opacity: 0; }
  .thumb-wrap:hover .mini-icon, .mini-icon:focus-visible { opacity: 1; }
  .mini-icon.armed { opacity: 1; }
  .pos { flex: none; width: 240px; max-width: 100%; }

  .row > .packs {
    flex: 1;
    min-width: 0;
  }
  .save-row > .tin,
  .save-row > .btn {
    height: calc(var(--space) * 8);
  }
  .rows > .row > .tin:not(.fixed) {
    flex: 1;
    min-width: 0;
  }

  .rows .tin.fixed,
  .rows :global(.segmented-control) {
    height: calc(var(--space) * 8);
  }
  .rows :global(.segmented-control) {
    align-items: center;
    width: 190px;
    box-sizing: border-box;
  }
  .rows :global(.segmented-control .seg-btn) {
    flex: 1 1 0;
    text-align: center;
    padding-left: 0;
    padding-right: 0;
  }

  .rows > .row > :global(.cf) { flex: 1; min-width: 0; }
  .panel-wrap { display: flex; flex: 1; min-width: 0; }
  .panel-wrap > :global(.cf) { flex: 1; min-width: 0; }
  .mini {
    flex: none;
    padding: var(--space) calc(var(--space) * 3);
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .mini:hover {
    color: var(--text);
    background: var(--hover);
  }

  .actions-row-end { display: flex; justify-content: flex-end; }
</style>
