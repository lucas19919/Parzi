<script lang="ts">
  /**
   * The same model menu the composer uses: provider rail, search, starred,
   * Smart Auto. Roles and the omnibar both bind `value` as `provider/model`.
   */
  import { createEventDispatcher, onMount } from "svelte";
  import { scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import Icon from "./Icon.svelte";
  import { api, type ModelRow } from "./api";
  import { updateProviderRow } from "./modelStore";
  import { portal } from "./portal";

  export let value = "";
  export let models: ModelRow[] = [];
  /** Accessible name for the trigger. */
  export let label = "Model";
  export let disabled = false;

  const dispatch = createEventDispatcher<{ change: { model: string } }>();

  const I = {
    chevD: "M6 9l6 6 6-6",
    search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
    spark: "M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4Z",
    model: "M4 4h16v16H4z",
  };

  const PROVIDER_ORDER = ["claude", "codex", "antigravity", "opencode", "xai"];
  const PROVIDER_NAME: Record<string, string> = {
    claude: "Claude",
    codex: "Codex",
    antigravity: "Antigravity",
    opencode: "OpenCode",
    xai: "Grok",
  };

  interface FamRow {
    provider: string;
    auth: string;
    family: string;
    label: string;
    value: string;
    legacy: boolean;
  }

  const AUTO_ROW: FamRow = {
    provider: "auto",
    auth: "ok",
    family: "auto",
    label: "Smart Auto",
    value: "auto",
    legacy: false,
  };

  let open = false;
  let railSel = "";
  let query = "";
  let index = 0;
  let btn: HTMLButtonElement | null = null;
  let searchEl: HTMLInputElement | null = null;
  let popStyle = "";
  let below = false;
  let favorites: string[] = [];
  let refreshing: string | null = null;

  $: allFams = (() => {
    const seen = new Map<string, FamRow>();
    for (const r of models) {
      for (const m of r.models) {
        const family = m.family || m.id;
        const key = `${r.provider}/${family}`;
        if (!seen.has(key)) {
          seen.set(key, {
            provider: r.provider,
            auth: r.auth,
            family,
            label: m.family_name || m.name || m.id,
            value: `${r.provider}/${family}`,
            legacy: !!m.legacy,
          });
        }
      }
    }
    return [...seen.values()];
  })();

  $: q = query.trim().toLowerCase();
  $: favRows = favorites
    .map((v) => allFams.find((r) => r.value === v))
    .filter((r): r is FamRow => !!r);

  $: items = ((): FamRow[] => {
    const match = (row: FamRow) =>
      !q ||
      row.label.toLowerCase().includes(q) ||
      row.provider.toLowerCase().includes(q) ||
      (PROVIDER_NAME[row.provider] ?? "").toLowerCase().includes(q) ||
      row.family.toLowerCase().includes(q);
    if (q) return [AUTO_ROW, ...allFams].filter(match);
    if (railSel === "__auto") return [AUTO_ROW];
    if (railSel === "__fav") return [...favRows];
    if (!railSel) return [];
    return allFams.filter((r) => r.provider === railSel);
  })();
  $: if (index >= items.length && items.length) index = 0;

  $: shown = (() => {
    if (!value || value === "auto") return { provider: "auto", name: "Smart Auto" };
    for (const r of models) {
      for (const m of r.models) {
        if (`${r.provider}/${m.id}` === value || `${r.provider}/${m.family}` === value) {
          return { provider: r.provider, name: m.family_name || m.name || m.id };
        }
      }
    }
    const [p, ...rest] = value.split("/");
    return { provider: p || "other", name: rest.join("/") || value };
  })();

  function statusOf(r: ModelRow): { kind: "sub" | "key" | "off" | "expired" } {
    if (r.auth === "expired") return { kind: "expired" };
    if (r.auth !== "ok") return { kind: "off" };
    if (r.billing === "subscription") return { kind: "sub" };
    return { kind: "key" };
  }

  function planSub(row: FamRow): string {
    if (row.provider === "auto") return "Routes across your signed-in providers";
    const parts: string[] = [PROVIDER_NAME[row.provider] ?? row.provider];
    const pr = models.find((r) => r.provider === row.provider);
    const plan =
      pr?.account ||
      (pr?.billing === "subscription" ? "Subscription" : pr?.billing === "api_key" ? "API key" : "");
    if (plan) parts.push(plan);
    if (row.legacy) parts.push("legacy");
    return parts.join(" · ");
  }

  function place() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const w = 440;
    const left = Math.max(8, Math.min(r.left, window.innerWidth - w - 8));
    const spaceAbove = r.top - 46;
    const spaceBelow = window.innerHeight - r.bottom - 8;
    if (spaceAbove >= 300 || spaceAbove >= spaceBelow) {
      const maxH = Math.max(180, Math.min(spaceAbove, 560));
      const bottom = Math.max(8, window.innerHeight - r.top + 8);
      popStyle = `left:${Math.round(left)}px;bottom:${Math.round(bottom)}px;max-height:${Math.round(maxH)}px;`;
      below = false;
    } else {
      popStyle = `left:${Math.round(left)}px;top:${Math.round(r.bottom + 8)}px;max-height:${Math.round(spaceBelow)}px;`;
      below = true;
    }
  }

  async function refreshProvider(p: string) {
    if (!p || p.startsWith("__")) return;
    refreshing = p;
    try {
      const row = await api.refreshProvider(p);
      updateProviderRow(row);
    } catch {
      /* keep cached rows */
    } finally {
      if (refreshing === p) refreshing = null;
    }
  }

  function selectRail(p: string) {
    railSel = p;
    query = "";
    index = 0;
    if (p && !p.startsWith("__")) void refreshProvider(p);
    setTimeout(() => searchEl?.focus(), 30);
  }

  function openPicker() {
    if (disabled) return;
    open = true;
    query = "";
    index = 0;
    const [p] = value.split("/");
    railSel =
      !value || value === "auto"
        ? "__auto"
        : PROVIDER_ORDER.includes(p)
          ? p
          : (PROVIDER_ORDER.find((id) => models.some((r) => r.provider === id)) ?? "__auto");
    if (railSel && !railSel.startsWith("__")) void refreshProvider(railSel);
    place();
    api.getConfig().then((c) => { favorites = c.favorite_models ?? []; }).catch(() => {});
    setTimeout(() => searchEl?.focus(), 30);
  }

  function pick(row: FamRow) {
    const openServe = row.provider === "opencode";
    if (row.auth !== "ok" && !openServe) {
      dispatch("change", { model: value });
      open = false;
      return;
    }
    value = row.value === "auto" ? "" : row.value;
    open = false;
    dispatch("change", { model: value });
  }

  async function toggleFav(spec: string) {
    try {
      favorites = await api.toggleFavorite(spec);
    } catch {}
  }

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.stopPropagation();
      open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      index = (index + 1) % Math.max(1, items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = (index - 1 + Math.max(1, items.length)) % Math.max(1, items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const row = items[index];
      if (row) pick(row);
    }
  }

  function onWinClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    if (open && !el.closest(".mp")) open = false;
  }

  onMount(() => {
    const onResize = () => { if (open) place(); };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  });
</script>

<svelte:window on:keydown={onKey} on:click={onWinClick} />

<div class="mp">
  <button
    bind:this={btn}
    class="trig"
    class:open
    {disabled}
    aria-label={label}
    aria-expanded={open}
    on:click|stopPropagation={() => (open ? (open = false) : openPicker())}
  >
    {#if shown.provider === "auto"}
      <span class="auto"><Icon d={I.spark} size={13} /></span>
    {:else if hasMark(shown.provider)}
      <ProviderLogo provider={shown.provider} size={13} />
    {:else}
      <span class="glyph"><Icon d={I.model} size={13} /></span>
    {/if}
    <span class="nm">{shown.name}</span>
    <span class="chev"><Icon d={I.chevD} size={11} /></span>
  </button>

  {#if open}
    <div
      use:portal
      class="pop"
      class:from-top={below}
      style={popStyle}
      transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}
      on:click|stopPropagation
    >
      <div class="search">
        <Icon d={I.search} size={13} />
        <input
          bind:this={searchEl}
          placeholder={q ? "Search all models…" : railSel === "__fav" ? "Starred models" : railSel === "__auto" ? "Smart Auto routing" : `Search ${(PROVIDER_NAME[railSel] ?? railSel)} models…`}
          bind:value={query}
        />
        {#if refreshing && refreshing === railSel}<span class="spin" />{/if}
      </div>
      <div class="body">
        <div class="rail">
          <button class="rail-row" class:on={railSel === "__auto"} title="Smart Auto" on:click={() => selectRail("__auto")}>
            <span class="auto"><Icon d={I.spark} size={15} /></span>
          </button>
          {#if favRows.length}
            <button class="rail-row" class:on={railSel === "__fav"} title="Starred" on:click={() => selectRail("__fav")}>
              <span class="star-rail">★</span>
            </button>
          {/if}
          {#each PROVIDER_ORDER as p}
            {@const pr = models.find((r) => r.provider === p)}
            {#if pr}
              {@const st = statusOf(pr)}
              <button
                class="rail-row"
                class:on={railSel === p}
                class:dim={st.kind === "off" || st.kind === "expired"}
                title={PROVIDER_NAME[p] ?? p}
                on:click={() => selectRail(p)}
              >
                {#if hasMark(p)}
                  <ProviderLogo provider={p} size={18} />
                {:else}
                  <span class="ini">{(p[0] ?? "?").toUpperCase()}</span>
                {/if}
              </button>
            {/if}
          {/each}
        </div>
        <div class="list">
          {#each items as row, i (row.value)}
            <button
              class="mrow"
              class:on={row.value === value || (!value && row.value === "auto") || i === index}
              on:click={() => pick(row)}
              on:mousemove={() => (index = i)}
            >
              {#if q && hasMark(row.provider)}
                <ProviderLogo provider={row.provider} size={14} />
              {/if}
              <span class="meta">
                <span class="name">{row.label}</span>
                <span class="sub">{planSub(row)}</span>
              </span>
              {#if row.auth !== "ok" && row.provider !== "opencode"}
                <span class="nokey">{row.auth === "expired" ? "expired" : "sign in"}</span>
              {/if}
              {#if row.provider !== "auto"}
                <span
                  class="star"
                  class:on={favorites.includes(row.value)}
                  role="button"
                  tabindex="0"
                  title="star model"
                  on:click|stopPropagation={() => toggleFav(row.value)}
                  on:keydown={(e) => e.key === "Enter" && toggleFav(row.value)}
                >{favorites.includes(row.value) ? "★" : "☆"}</span>
              {/if}
            </button>
          {/each}
          {#if !items.length}
            <div class="empty">No models — Settings › Models</div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .mp { position: relative; min-width: 0; width: 100%; }
  .trig {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 13px; padding: 7px 10px; cursor: pointer; text-align: left;
  }
  .trig:hover, .trig.open { border-color: var(--accent-line); }
  .trig:disabled { opacity: 0.5; cursor: default; }
  .nm { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chev { color: var(--text-4); flex: none; display: inline-flex; }
  .auto { display: inline-flex; color: var(--accent-text); flex: none; }
  .glyph { display: inline-flex; color: var(--text-3); flex: none; }

  .pop {
    position: fixed; z-index: 80; width: 440px; max-width: calc(100vw - 16px);
    height: 430px; max-height: calc(100vh - 120px);
    display: flex; flex-direction: column;
    background: var(--menu); border: 1px solid var(--line); border-radius: var(--glass-radius);
    box-shadow: var(--menu-shadow); overflow: hidden; transform-origin: bottom left;
  }
  .pop.from-top { transform-origin: top left; }
  .search {
    display: flex; align-items: center; gap: 8px; padding: 8px 10px; flex: none;
    color: var(--text-3); border-bottom: 1px solid var(--line-2);
  }
  .search input {
    flex: 1; min-width: 0; background: transparent; border: none; outline: none;
    color: var(--text); font: inherit; font-size: 13px;
  }
  .search input::placeholder { color: var(--text-4); }
  .spin {
    flex: none; width: 11px; height: 11px; border-radius: 50%;
    border: 1.6px solid var(--line-3); border-top-color: var(--text-2);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .body { display: flex; flex: 1; min-height: 0; }
  .rail {
    flex: none; width: 54px; display: flex; flex-direction: column; gap: 2px;
    padding: 6px 5px; overflow-y: auto; border-right: 1px solid var(--line-2);
  }
  .rail-row {
    display: flex; align-items: center; justify-content: center; width: 100%; height: 36px;
    background: none; border: none; border-radius: 8px; color: var(--text-2); cursor: pointer; padding: 0;
  }
  .rail-row:hover { background: var(--surface-2); color: var(--text); }
  .rail-row.on { background: var(--surface-3); color: var(--text); }
  .rail-row.dim { opacity: 0.4; }
  .star-rail { color: var(--warn); font-size: 15px; line-height: 1; }
  .ini {
    width: 20px; height: 20px; display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-2); border-radius: 6px; font-size: 10px; font-weight: 600;
  }
  .list { flex: 1; min-width: 0; overflow-y: auto; padding: 5px; display: flex; flex-direction: column; gap: 1px; }
  .mrow {
    display: flex; align-items: center; gap: 9px; width: 100%;
    background: none; border: none; border-radius: 7px; color: var(--text-2);
    text-align: left; padding: 6px 8px; cursor: pointer; font: inherit; font-size: 13px;
  }
  .mrow:hover, .mrow.on { background: var(--surface-2); color: var(--text); }
  .meta { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 11px; color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nokey { font-size: 11px; color: var(--bad); flex: none; }
  .star { color: var(--text-4); font-size: 13px; padding: 1px 4px; cursor: pointer; flex: none; opacity: 0; }
  .mrow:hover .star, .star.on { opacity: 1; }
  .star.on { color: var(--warn); }
  .empty { padding: 14px; text-align: center; color: var(--text-3); font-size: 12px; }
</style>
