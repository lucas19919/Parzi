<script lang="ts">
  import { SLIDE } from "./motion";
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { covered, setOverlay, setPageArea } from "./overlay";
  import SuggestList from "./SuggestList.svelte";
  import { get } from "svelte/store";
  import type { Suggestion } from "./suggest";
  import { addressSuggest } from "./addressSuggest";
  import { bookmarks, history, pins } from "./browserData";
  import { prompts, removePrompt, type PagePrompt } from "./browserState";
  import { appFor } from "./pageRules";
  import { toAddress, type Tab } from "./tabs";
  import { tooltip } from "./tooltip";
  import { PHONES, phoneFit, phoneFor, viewport, type DeviceId, type DuoPose, type PhoneId } from "./devices";
  import PhoneFrame from "./PhoneFrame.svelte";
  import { NO_VEIL, stepVeil, type Veil } from "./veil";

  export let tab: Tab;
  export let immersive = false;
  /** Full screen over a session: offer laptop, iPhone, Android and the iPhone Duo. */
  export let devices = false;

  // A phone is emulated for real (screen, pixel density, user agent, touch)
  // and drawn as the phone, scaled down to the room the page has. The page
  // gets the screen inside its system bars and side rail.
  let device: DeviceId = "laptop";
  let duoPose: DuoPose = "open";
  let duoOpenPose: "open" | "portrait" = "open";
  const PHONE_IDS: PhoneId[] = ["iphone", "android", "duo"];
  let roomW = 0;
  let roomH = 0;
  $: phone = !immersive && devices && device !== "laptop" ? phoneFor(device, duoPose) : null;
  $: view = phone ? viewport(phone) : null;
  $: fit = phone ? phoneFit(phone, { width: roomW, height: roomH }) : 1;
  $: screenBg = tab.bg || "rgb(255, 255, 255)";
  // What the page is emulating, and what was last asked for ("" is the
  // laptop): a quick switch back cancels the switch still waiting.
  let emulated = "";
  let wanted = "";
  let emulateTimer: ReturnType<typeof setTimeout> | undefined;
  $: syncDevice(tab.id, phone && view ? { width: view.width, height: view.height, scale: phone.scale, user_agent: phone.userAgent, fit } : null);
  function syncDevice(id: string, d: Parameters<typeof api.browserEmulate>[1]) {
    const key = d ? `${id}:${JSON.stringify(d)}` : "";
    if (key === wanted) return;
    wanted = key;
    clearTimeout(emulateTimer);
    if (key === emulated) return;
    emulateTimer = setTimeout(() => {
      api
        .browserEmulate(id, d)
        .then(() => {
          if (wanted === key) emulated = key;
        })
        .catch((e) => {
          if (wanted === key) wanted = emulated;
          dispatch("error", { text: String(e) });
        });
    }, 150);
  }
  onMount(() => () => clearTimeout(emulateTimer));

  const dispatch = createEventDispatcher<{
    navigate: { url: string };
    error: { text: string };
    /** A blocked pop-up the person chose to open: it takes this page's place. */
    openTab: { url: string };
    recover: void;
    resume: void;
  }>();

  let slot: HTMLDivElement | null = null;
  let field: HTMLInputElement | null = null;
  let address = tab.url ?? "";
  let editing = false;
  let frame = 0;
  let alive = true;
  let reported = false;
  let chain: Promise<void> = Promise.resolve();
  let form: HTMLFormElement | null = null;
  const suggest = addressSuggest({
    sources: () => ({ visits: get(history), marks: [...get(bookmarks), ...get(pins)] }),
    phrases: (q) => api.searchSuggest(q),
    live: () => editing,
  });
  let snapshot = "";
  let snapAt = 0;
  let shownNow = false;
  let veil: Veil = NO_VEIL;
  let freshTimer: number | null = null;
  let listStyle = "";

  $: url = tab.url ?? "";
  $: if (!editing) address = url;
  // Fresh navigations veil behind a blurred snapshot of the page being left
  // until the new one loads (or a failsafe timeout) so a blank webview never
  // flashes in. Keep this above `veiled`: the order of these statements is
  // the order Svelte runs them in.
  $: trackLoad(url, !!tab.loading);
  $: veiled = !!url && !tab.crashed && !!tab.loading && veil.fresh;
  $: secure = url.startsWith("https://");
  $: url, immersive, $covered, slot, tab.crashed, veiled, phone, fit, schedule();
  $: open = editing && $suggest.rows.length > 0;
  $: setOverlay(`address-${tab.id}`, open);
  $: if (open && form) listStyle = `left:${form.offsetLeft}px;width:${form.offsetWidth}px`;
  $: prompt = immersive ? null : ($prompts.find((p) => p.tab === tab.id) ?? null);

  function trackLoad(now: string, loading: boolean) {
    const step = stepVeil(veil, now, loading);
    veil = step.veil;
    if (step.started) {
      if (freshTimer) clearTimeout(freshTimer);
      freshTimer = window.setTimeout(() => {
        veil = { ...veil, fresh: false };
        freshTimer = null;
      }, 8000);
    }
    if (step.finished) {
      if (freshTimer) clearTimeout(freshTimer);
      freshTimer = null;
      // The snapshot pictures a page that is gone now; the next cover takes a new one.
      snapshot = "";
      snapAt = 0;
    }
  }

  export async function focusAddress() {
    await tick();
    field?.focus();
    field?.select();
  }

  function schedule() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      chain = chain.then(sync).catch(() => {});
    });
  }

  function freshSnapshot(): Promise<void> {
    if (!shownNow || Date.now() - snapAt < 1500) return Promise.resolve();
    snapAt = Date.now();
    const shot = api
      .browserSnapshot(tab.id)
      .then((s) => {
        if (alive) snapshot = s;
      })
      .catch(() => {});
    return Promise.race([shot, new Promise<void>((done) => setTimeout(done, 300))]);
  }

  async function sync() {
    if (!alive || !slot) return;
    if ($covered && url && !tab.crashed) await freshSnapshot();
    if (!alive || !slot) return;
    const r = slot.getBoundingClientRect();
    try {
      if ($covered || tab.crashed || veiled || !url || r.width < 8 || r.height < 8) {
        shownNow = false;
        setPageArea(null);
        await api.browserHide();
        return;
      }
      const x = Math.floor(r.x);
      const y = Math.floor(r.y);
      const rect = { x, y, width: Math.ceil(r.x + r.width) - x, height: Math.ceil(r.y + r.height) - y };
      await api.browserShow(tab.id, rect, url, tab.owner ?? null);
      shownNow = true;
      setPageArea({ left: rect.x, top: rect.y, right: rect.x + rect.width, bottom: rect.y + rect.height });
    } catch (e) {
      if (!reported) {
        reported = true;
        dispatch("error", { text: String(e) });
      }
    }
  }

  function startEditing() {
    editing = true;
    suggest.reset();
    field?.select();
    if (url && shownNow) {
      snapAt = Date.now();
      api
        .browserSnapshot(tab.id)
        .then((s) => alive && (snapshot = s))
        .catch(() => {});
    }
  }

  function stopEditing() {
    editing = false;
    suggest.reset();
  }

  async function onAddressInput(e: Event) {
    if (!field) return;
    const inserting = !!(e as InputEvent).inputType?.startsWith("insert") && field.selectionStart === field.value.length;
    const shown = suggest.type(field.value, inserting);
    if (shown.select) {
      address = shown.text;
      await tick();
      field?.setSelectionRange(...shown.select);
    }
  }

  function leaveFor(next: string) {
    stopEditing();
    field?.blur();
    dispatch("navigate", { url: next });
    api.browserFocus(tab.id).catch(() => {});
  }

  function pick(row: Suggestion) {
    leaveFor(row.url);
  }

  function go() {
    const row = suggest.chosen();
    if (row) {
      pick(row);
      return;
    }
    const next = toAddress(address);
    if (next) leaveFor(next);
  }

  function nav(action: "back" | "forward" | "reload" | "stop" | "zoom-reset") {
    api.browserNav(tab.id, action).catch((e) => dispatch("error", { text: String(e) }));
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const text = suggest.move(e.key === "ArrowDown" ? 1 : -1);
      if (text !== null) {
        e.preventDefault();
        address = text;
      }
      return;
    }
    if (e.key === "Escape") {
      const typed = suggest.escape();
      if (typed !== null) {
        address = typed || url;
        return;
      }
      stopEditing();
      address = url;
      field?.blur();
    }
  }

  function answer(p: PagePrompt, yes: boolean) {
    removePrompt(p.id);
    if (p.kind === "external") api.browserExternal(p.id, yes).catch((e) => dispatch("error", { text: String(e) }));
    else if (yes && p.url) dispatch("openTab", { url: p.url });
  }

  let stage: HTMLDivElement | null = null;

  onMount(() => {
    const ro = new ResizeObserver(schedule);
    if (slot) ro.observe(slot);
    if (stage) ro.observe(stage);
    window.addEventListener("resize", schedule);
    if (!url) void focusAddress();
    return () => {
      alive = false;
      suggest.reset();
      if (freshTimer) clearTimeout(freshTimer);
      setOverlay(`address-${tab.id}`, false);
      setPageArea(null);
      cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", schedule);
    };
  });
</script>

<div class="page">
  {#if !immersive}
    <div class="toolbar">
      <button class="nav" use:tooltip={"Back (Alt+Left)"} disabled={!tab.canGoBack} on:click={() => nav("back")}>
        <Icon name="arrowLeft" size={14} />
      </button>
      <button class="nav" use:tooltip={"Forward (Alt+Right)"} disabled={!tab.canGoForward} on:click={() => nav("forward")}>
        <Icon name="arrowRight" size={14} />
      </button>
      {#if tab.loading}
        <button class="nav" use:tooltip={"Stop loading"} on:click={() => nav("stop")}><Icon name="close" size={14} /></button>
      {:else}
        <button class="nav" use:tooltip={"Reload (Ctrl+R)"} disabled={!url} on:click={() => nav("reload")}>
          <Icon name="reload" size={14} />
        </button>
      {/if}
      <form bind:this={form} class="address" class:editing on:submit|preventDefault={go}>
        {#if url && !editing}
          <span class="scheme" class:secure title={secure ? "Secure connection" : "Not secure"}>
            <Icon name={secure ? "lock" : "unlock"} size={12} />
          </span>
        {/if}
        <input
          bind:this={field}
          bind:value={address}
          on:focus={startEditing}
          on:blur={stopEditing}
          on:keydown={onKey}
          on:input={onAddressInput}
          placeholder="Search or enter an address"
          aria-label="Address and search"
          spellcheck="false"
          autocomplete="off"
        />
        {#if tab.zoom}

          <button type="button" class="zoom" use:tooltip={"Reset zoom (Ctrl+0)"} on:click={() => nav("zoom-reset")}>
            {Math.round(tab.zoom * 100)}%
          </button>
        {/if}
        {#if tab.loading}<span class="progress" />{/if}
      </form>
      {#if devices}
        <div class="devices">
          <button class="nav" class:on={device === "laptop"} aria-pressed={device === "laptop"} use:tooltip={"As on a laptop"} on:click={() => (device = "laptop")}>
            <Icon name="laptop" size={14} />
          </button>
          {#each PHONE_IDS as id (id)}
            <button class="nav label" class:on={device === id} aria-pressed={device === id} use:tooltip={PHONES[id].tip} on:click={() => (device = id)}>
              <Icon name={id === "duo" ? "duo" : "phone"} size={14} />{PHONES[id].label}
            </button>
          {/each}
        </div>
        {#if device === "duo"}
          <div class="devices" data-ui-segmented>
            <button
              class="nav label"
              class:on={duoPose !== "closed"}
              aria-pressed={duoPose !== "closed"}
              use:tooltip={"Unfolded: 7.6-inch inner screen"}
              on:click={() => (duoPose = duoOpenPose)}>Open</button
            >
            <button
              class="nav label"
              class:on={duoPose === "closed"}
              aria-pressed={duoPose === "closed"}
              use:tooltip={"Folded: 5.4-inch outer screen"}
              on:click={() => (duoPose = "closed")}>Closed</button
            >
            {#if duoPose !== "closed"}
              <button
                class="nav"
                class:on={duoPose === "portrait"}
                aria-pressed={duoPose === "portrait"}
                aria-label="Rotate inner screen"
                use:tooltip={duoPose === "portrait" ? "Rotate to landscape (951x669)" : "Rotate to portrait (669x951)"}
                on:click={() => {
                  duoOpenPose = duoPose === "portrait" ? "open" : "portrait";
                  duoPose = duoOpenPose;
                }}
              >
                <Icon name="rotate" size={14} />
              </button>
            {/if}
          </div>
        {/if}
      {/if}
      <button
        class="nav"
        use:tooltip={"Open outside Parzi, in the system browser"}
        disabled={!/^https?:\/\//i.test(url)}
        on:click={() => api.openConfirmedUrl(url).catch((e) => dispatch("error", { text: String(e) }))}
      >
        <Icon name="external" size={14} />
      </button>
    </div>
  {/if}
  {#if prompt}
    {@const p = prompt}

    <div class="infobar" role="status">
      {#if p.kind === "external"}
        <Icon name="external" />
        <span class="ask">Open this link in {appFor(p.url ?? "")}? <span class="link">{p.url}</span></span>
        <button class="btn" on:click={() => answer(p, false)}>Cancel</button>
        <button class="btn primary" on:click={() => answer(p, true)}>Open</button>
      {:else}
        <Icon name="browser" />
        <span class="ask">Blocked a pop-up <span class="link">{p.url}</span></span>
        {#if /^https?:\/\//i.test(p.url ?? "")}
          <button class="btn" on:click={() => answer(p, true)}>Open</button>
        {/if}
        <button class="nav" use:tooltip={"Dismiss"} on:click={() => answer(p, false)}><Icon name="close" /></button>
      {/if}
    </div>
  {/if}
  {#if open && !immersive}
    <div class="suggest" style={listStyle}>
      <SuggestList rows={$suggest.rows} active={$suggest.active} typed={$suggest.typed} on:pick={(e) => pick(e.detail.row)} on:hover={(e) => suggest.hover(e.detail.index)} />
    </div>
  {/if}
  <div class="stage" class:framed={!!phone} bind:this={stage} bind:clientWidth={roomW} bind:clientHeight={roomH}>
  <PhoneFrame {phone} {fit} bg={screenBg}>
  <div
    class="slot"
    class:phone={!!phone}
    style:width={view ? `${view.width * fit}px` : null}
    style:height={view ? `${view.height * fit}px` : null}
    class:empty={!url && !phone}
    class:dark={immersive && !phone}
    style={phone ? `background:${screenBg}` : !immersive && tab.bg && !tab.crashed ? `background:${tab.bg}` : ""}
    bind:this={slot}
  >
    {#if !url}
      <div class="blank">Type an address or a search above</div>
    {:else if tab.crashed}
      <div class="crash">
        <Icon name="alert" />
        <p class="crash-title t-page">{tab.crashed === "unresponsive" ? "This page isn't responding" : "This page crashed"}</p>
        <p class="crash-hint">
          {tab.crashed === "unresponsive" ? "You can wait for it or reload it." : "Reload it to pick up where it was."}
        </p>
        <div class="crash-actions">
          {#if tab.crashed === "unresponsive"}
            <button class="btn" on:click={() => dispatch("resume")}>Wait</button>
          {/if}
          <button class="btn primary" on:click={() => dispatch("recover")}>Reload</button>
        </div>
      </div>
    {:else if ($covered || open) && snapshot}
      <img class="snap" src={snapshot} alt="" draggable="false" />
    {:else if veiled}
      <div class="loading" transition:fade={{ duration: SLIDE }}>
        {#if snapshot}<img class="snap blur" src={snapshot} alt="" draggable="false" />{/if}
      </div>
    {/if}
  </div>
  </PhoneFrame>
  </div>
</div>

<style>
  .zoom {
    flex: none;
    height: 22px;
    margin-right: var(--space);
    padding: 0 calc(var(--space) * 2);
    background: var(--hover);
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .zoom:hover {
    color: var(--text);
  }
  .page {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space);
    min-height: 26px;
    margin: var(--gap) var(--gap) var(--gap);
    padding: 0 var(--space);
  }
  .nav {
    width: 26px;
    height: 26px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .nav:hover:not(:disabled) {
    background: var(--line);
    color: var(--text);
  }
  .nav:disabled {
    color: var(--muted);
    cursor: default;
  }
  .nav.on {
    color: var(--accent);
  }
  .address {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 26px;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    padding: 0 calc(var(--space) * 3);
    background: var(--panel);
    border: 1px solid transparent;
    border-radius: var(--radius);
    overflow: hidden;
  }
  .address:hover {
    background: var(--line);
  }
  .address.editing {
    background: var(--bg);
    border-color: var(--accent);
  }
  .scheme {
    display: inline-flex;
    flex: none;
    color: var(--warn-text);
  }
  .scheme.secure {
    color: var(--muted);
  }
  .address input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .address input::placeholder {
    color: var(--muted);
  }
  .progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 30%;
    background: var(--accent);
    animation: sweep 1.1s ease-in-out infinite;
  }
  .infobar {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: 44px;
    padding: calc(var(--space) * 2) calc(var(--space) * 2) calc(var(--space) * 2) calc(var(--space) * 4);
    border-bottom: 1px solid var(--line);
    color: var(--muted);
    font-size: var(--fs-md);
  }
  .ask {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .link {
    margin-left: var(--space);
    color: var(--muted);
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: var(--gap);
    margin: 0 var(--gap) var(--gap);
  }
  /* A phone: centred, drawn by PhoneFrame. The page is a sharp rectangle
     exactly its viewport times the fit, since the native page view is sized
     to it; the phone's bars hide nothing. */
  .stage.framed {
    align-items: center;
    justify-content: center;
  }
  .slot.phone {
    flex: none;
    border: none;
    border-radius: 0;
  }
  .nav.label {
    width: auto;
    gap: var(--space);
    padding: 0 var(--space);
    font-size: var(--fs-sm);
  }
  .devices {
    display: flex;
    gap: var(--space);
  }
  .slot {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: var(--radius-in);
    overflow: hidden;
    background: var(--bg);
  }
  .slot.dark {
    background: black;
  }
  .snap {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: fill;
  }
  .loading {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: var(--bg);
  }
  .loading .blur {
    filter: blur(18px) saturate(0.9);
    transform: scale(1.06);
    opacity: 0.75;
  }
  .loading::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(100deg, transparent 30%, color-mix(in srgb, var(--text) 5%, transparent) 50%, transparent 70%);
    background-size: 200% 100%;
    animation: load-sweep 1.6s ease-in-out infinite;
    pointer-events: none;
  }
  @keyframes load-sweep {
    from {
      background-position: 180% 0;
    }
    to {
      background-position: -80% 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .loading::after {
      animation: none;
    }
  }
  .crash {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 6);
    color: var(--muted);
    text-align: center;
  }
  .crash-title {
    margin: var(--space) 0 0;
  }
  .crash-hint {
    margin: 0;
    font-size: var(--fs-md);
  }
  .crash-actions {
    display: flex;
    gap: calc(var(--space) * 2);
    margin-top: calc(var(--space) * 2);
  }
  .suggest {
    position: absolute;
    top: calc(var(--gap) + 26px);
    z-index: 20;
    min-width: 320px;
  }
  .slot.empty {
    background: transparent;
  }
  .blank {
    font-size: var(--fs-md);
    color: var(--muted);
  }
</style>
