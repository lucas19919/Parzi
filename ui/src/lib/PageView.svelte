<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { adblock, api, vault, BITWARDEN_INSTALL, type AdblockState, type VaultLogin } from "./api";
  import { popover, placeAbove } from "./popover";
  import { toast } from "./toast";
  import { covered, setOverlay } from "./overlay";
  import SuggestList from "./SuggestList.svelte";
  import { boxText, firstRows, looksLikeUrl, mergeRows, pageRows, phraseRows, type Suggestion } from "./suggest";
  import { addPin, bookmarks, completeAddress, history, pins, removePin } from "./browserData";
  import { toAddress, type Tab } from "./tabs";

  export let tab: Tab;
  export let immersive = false;

  const dispatch = createEventDispatcher<{ navigate: { url: string }; error: { text: string } }>();

  let slot: HTMLDivElement | null = null;
  let field: HTMLInputElement | null = null;
  let address = tab.url ?? "";
  let editing = false;
  let frame = 0;
  let alive = true;
  let reported = false;
  let chain: Promise<void> = Promise.resolve();
  let shield: AdblockState | null = null;
  let keyBtn: HTMLButtonElement | null = null;
  let choices: VaultLogin[] = [];
  let choiceStyle = "";
  let filling = false;
  let form: HTMLFormElement | null = null;
  let rows: Suggestion[] = [];
  let active = -1;
  let typed = "";
  let snapshot = "";
  let remoteSeq = 0;
  let remoteTimer = 0;
  let listStyle = "";

  $: url = tab.url ?? "";
  $: if (!editing) address = url;
  $: secure = url.startsWith("https://");
  $: pinned = $pins.some((p) => p.url === url);
  $: url, immersive, $covered, slot, schedule();
  $: void loadShield(hostOf(url));
  $: open = editing && rows.length > 0;
  $: setOverlay(`address-${tab.id}`, open);
  $: if (open && form) listStyle = `left:${form.offsetLeft}px;width:${form.offsetWidth}px`;

  function hostOf(u: string) {
    try {
      return new URL(u).hostname;
    } catch {
      return "";
    }
  }

  async function loadShield(host: string) {
    shield = host ? await adblock.state(url).catch(() => null) : null;
  }

  async function toggleShield() {
    if (!shield || !url) return;
    try {
      const blocking = shield.enabled && !shield.allowed;
      shield = await adblock.site(url, blocking);
      nav("reload");
    } catch (e) {
      dispatch("error", { text: String(e) });
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

  async function sync() {
    if (!alive || !slot) return;
    const r = slot.getBoundingClientRect();
    try {
      if ($covered || !url || r.width < 8 || r.height < 8) {
        await api.browserHide();
        return;
      }
      // Overscan outward: rounding inward leaves a 1px sliver of the
      // Parzi background around the page, visible as an outline.
      const x = Math.floor(r.x);
      const y = Math.floor(r.y);
      const rect = { x, y, width: Math.ceil(r.x + r.width) - x, height: Math.ceil(r.y + r.height) - y };
      await api.browserShow(tab.id, rect, url);
    } catch (e) {
      if (!reported) {
        reported = true;
        dispatch("error", { text: String(e) });
      }
    }
  }

  async function fillLogin() {
    if (!url || filling) return;
    filling = true;
    try {
      let found = await vault.logins(tab.id);
      if (found.status === "missing") {
        toast(`Install the Bitwarden CLI first: ${BITWARDEN_INSTALL}`, true);
        return;
      }
      if (found.status !== "unlocked") {
        toast("Unlock Bitwarden in the window that just opened");
        await vault.unlock();
        found = await vault.logins(tab.id);
        if (found.status !== "unlocked") return;
      }
      if (!found.items.length) toast(`No saved login for ${found.host}`);
      else if (found.items.length === 1) await fillWith(found.items[0]);
      else {
        choices = found.items;
        if (keyBtn) choiceStyle = placeAbove(keyBtn, 280, true);
      }
    } catch (e) {
      const text = String(e);
      toast(text === "missing" ? `Install the Bitwarden CLI first: ${BITWARDEN_INSTALL}` : text, true);
    } finally {
      filling = false;
    }
  }

  async function fillWith(login: VaultLogin) {
    choices = [];
    try {
      const result = await vault.fill(tab.id, login.id);
      if (result === "none") toast("No login form found on this page");
      else if (result === "moved") toast("The page changed; try again");
    } catch (e) {
      toast(String(e), true);
    }
  }

  function startEditing() {
    editing = true;
    typed = "";
    rows = [];
    active = -1;
    field?.select();
    snapshot = "";
    if (url) api.browserSnapshot(tab.id).then((s) => (snapshot = s)).catch(() => {});
  }

  function stopEditing() {
    editing = false;
    rows = [];
    active = -1;
    clearTimeout(remoteTimer);
  }

  async function onAddressInput(e: Event) {
    if (!field) return;
    const value = field.value;
    typed = value;
    active = -1;
    let completed = "";
    if ((e as InputEvent).inputType?.startsWith("insert") && field.selectionStart === value.length) {
      const full = completeAddress(value, $history, [...$bookmarks, ...$pins]);
      if (full) {
        completed = value + full.slice(value.length);
        address = completed;
        await tick();
        field?.setSelectionRange(value.length, completed.length);
      }
    }
    suggest(value, completed);
  }

  function suggest(value: string, completed: string) {
    const marks = [...$bookmarks, ...$pins];
    rows = mergeRows(firstRows(value, completed), pageRows(value, $history, marks));
    clearTimeout(remoteTimer);
    if (!value.trim() || looksLikeUrl(value)) return;
    const seq = ++remoteSeq;
    remoteTimer = window.setTimeout(async () => {
      const phrases = await api.searchSuggest(value).catch(() => [] as string[]);
      if (seq !== remoteSeq || !editing) return;
      rows = mergeRows(firstRows(value, completed), phraseRows(value, phrases), pageRows(value, $history, marks));
    }, 120);
  }

  function pick(row: Suggestion) {
    stopEditing();
    field?.blur();
    dispatch("navigate", { url: row.url });
  }

  function go() {
    if (active >= 0 && rows[active]) {
      pick(rows[active]);
      return;
    }
    const next = toAddress(address);
    if (!next) return;
    stopEditing();
    field?.blur();
    dispatch("navigate", { url: next });
  }

  function nav(action: "back" | "forward" | "reload" | "stop") {
    api.browserNav(tab.id, action).catch((e) => dispatch("error", { text: String(e) }));
  }

  function onKey(e: KeyboardEvent) {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && rows.length) {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      active = active + step < -1 ? rows.length - 1 : active + step >= rows.length ? -1 : active + step;
      address = active >= 0 ? boxText(rows[active]) : typed;
      return;
    }
    if (e.key === "Escape") {
      if (rows.length) {
        rows = [];
        active = -1;
        address = typed || url;
        return;
      }
      stopEditing();
      address = url;
      field?.blur();
    }
  }

  function togglePin() {
    if (pinned) removePin(url);
    else addPin(url, tab.title);
  }

  onMount(() => {
    const ro = new ResizeObserver(schedule);
    if (slot) ro.observe(slot);
    window.addEventListener("resize", schedule);
    if (!url) void focusAddress();
    return () => {
      alive = false;
      clearTimeout(remoteTimer);
      setOverlay(`address-${tab.id}`, false);
      cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", schedule);
    };
  });
</script>

<div class="page">
  {#if !immersive}
    <div class="toolbar">
      <button class="nav" title="Back (Alt+Left)" disabled={!tab.canGoBack} on:click={() => nav("back")}>
        <Icon name="arrowLeft" size={15} />
      </button>
      <button class="nav" title="Forward (Alt+Right)" disabled={!tab.canGoForward} on:click={() => nav("forward")}>
        <Icon name="arrowRight" size={15} />
      </button>
      {#if tab.loading}
        <button class="nav" title="Stop" on:click={() => nav("stop")}><Icon name="close" size={15} /></button>
      {:else}
        <button class="nav" title="Reload (Ctrl+R)" disabled={!url} on:click={() => nav("reload")}>
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
          spellcheck="false"
          autocomplete="off"
        />
        {#if tab.loading}<span class="progress" />{/if}
      </form>
      {#if shield}
        {@const blocking = shield.enabled && !shield.allowed}
        <button
          class="nav shield"
          class:off={!blocking}
          title={blocking
            ? `Blocking ads and trackers${tab.blocked ? ` (${tab.blocked} on this page)` : ""}. Click to allow them on ${shield.host}.`
            : shield.enabled
              ? `Ads allowed on ${shield.host}. Click to block them.`
              : "The ad blocker is off. Click to turn it on."}
          on:click={toggleShield}
        >
          <Icon name="shield" size={14} />
          {#if blocking && tab.blocked}<span class="count">{tab.blocked > 99 ? "99+" : tab.blocked}</span>{/if}
        </button>
      {/if}
      <button
        bind:this={keyBtn}
        class="nav"
        class:busy={filling}
        title="Fill a login from Bitwarden"
        disabled={!url.startsWith("https://")}
        on:click={fillLogin}
      >
        <Icon name="key" size={14} />
      </button>
      <button class="nav" class:on={pinned} title={pinned ? "Unpin from Home" : "Pin to Home"} disabled={!url} on:click={togglePin}>
        <Icon name="pin" size={14} />
      </button>
    </div>
  {/if}
  {#if open && !immersive}
    <div class="suggest" style={listStyle}>
      <SuggestList {rows} {active} typed={typed} on:pick={(e) => pick(e.detail.row)} on:hover={(e) => (active = e.detail.index)} />
    </div>
  {/if}
  {#if choices.length}
    <div class="logins" style={choiceStyle} use:popover={{ anchor: keyBtn, close: () => (choices = []) }}>
      {#each choices as login (login.id)}
        <button class="login" on:click={() => fillWith(login)}>
          <span class="login-name">{login.name}</span>
          {#if login.username}<span class="login-user">{login.username}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
  <div class="slot" class:empty={!url} class:dark={immersive} style={!immersive && tab.bg ? `background:${tab.bg}` : ""} bind:this={slot}>
    {#if !url}
      <div class="blank">Type an address or a search above</div>
    {:else if open && snapshot}
      <img class="snap" src={snapshot} alt="" draggable="false" />
    {/if}
  </div>
</div>

<style>
  .page {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px;
    border-bottom: 1px solid var(--line);
  }
  .nav {
    width: 30px;
    height: 30px;
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
    color: var(--faint);
    cursor: default;
  }
  .nav.on {
    color: var(--accent);
  }
  .nav.busy {
    opacity: 0.5;
  }
  .logins {
    width: 280px;
    padding: 5px;
    overflow-y: auto;
  }
  .login {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: 7px 10px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .login:hover {
    background: var(--line);
  }
  .login-name {
    font-size: 13px;
  }
  .login-user {
    font-size: 11.5px;
    color: var(--muted);
  }
  .shield {
    position: relative;
    width: auto;
    min-width: 30px;
    gap: 4px;
    padding: 0 7px;
    color: var(--accent);
  }
  .shield.off {
    color: var(--faint);
  }
  .count {
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .address {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 6px;
    padding: 0 10px;
    background: var(--panel);
    border: 1px solid transparent;
    border-radius: 999px;
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
    color: var(--warn);
  }
  .scheme.secure {
    color: var(--faint);
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
    font-size: 13px;
  }
  .address input::placeholder {
    color: var(--faint);
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
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .slot {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
  }
  .slot.dark {
    background: #000;
  }
  .snap {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: fill;
  }
  .suggest {
    position: absolute;
    top: 38px;
    z-index: 20;
    min-width: 320px;
  }
  .slot.empty {
    background: transparent;
  }
  .blank {
    font-size: 12.5px;
    color: var(--faint);
  }
</style>
