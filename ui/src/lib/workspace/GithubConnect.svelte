<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { hub, type GithubStatus } from "../api";

  export let status: GithubStatus | null = null;

  const dispatch = createEventDispatcher<{ connected: { status: GithubStatus } }>();

  let token = "";
  let busy = "";
  let err = "";

  onMount(async () => {
    if (status) return;
    try {
      status = await hub.githubStatus();
      if (status.connected) dispatch("connected", { status });
    } catch (e) {
      err = String(e);
    }
  });

  async function connect(paste: boolean) {
    busy = paste ? "token" : "gh";
    err = "";
    try {
      status = await hub.githubConnect(paste ? token.trim() : undefined);
      token = "";
      dispatch("connected", { status });
    } catch (e) {
      err = String(e);
    } finally {
      busy = "";
    }
  }
</script>

<div class="gh">
  {#if status?.connected}
    <div class="signed">
      <span class="dot" />
      <span>Signed in as <b>{status.login}</b></span>
      <span class="hint">Paste another token below to switch account.</span>
    </div>
  {/if}

  <label class="lab" for="gh-token">Personal access token</label>
  <div class="row">
    <input
      id="gh-token"
      class="in"
      type="password"
      placeholder="ghp_… (scope: repo, read:org)"
      autocomplete="off"
      bind:value={token}
      on:keydown={(e) => { if (e.key === "Enter" && token.trim()) { e.stopPropagation(); connect(true); } }}
    />
    <button class="btn primary" disabled={!token.trim() || !!busy} on:click={() => connect(true)}>
      {busy === "token" ? "Checking…" : "Connect"}
    </button>
  </div>
  <p class="note">
    Parzi has no sign-in server of its own — the token stays in this machine's
    keyring and is used only to list and clone your repos.
  </p>

  {#if status?.gh}
    <div class="or"><span>or</span></div>
    <button class="btn" disabled={!!busy} on:click={() => connect(false)}>
      {busy === "gh" ? "Asking gh…" : "Use the token gh already has"}
    </button>
  {/if}

  {#if err}<div class="err">{err}</div>{/if}
</div>

<style>
  .gh { display: flex; flex-direction: column; gap: 8px; }
  .signed {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    background: var(--ok-soft); border: 1px solid var(--ok-line);
    border-radius: var(--radius-3); padding: 9px 12px; font-size: 12.5px; color: var(--text);
  }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); flex: none; }
  .hint { color: var(--text-3); font-size: 11.5px; }
  .lab { font-size: 11.5px; color: var(--text-3); }
  .row { display: flex; gap: 8px; }
  .in {
    flex: 1; min-width: 0; background: var(--input); border: 1px solid var(--line-2);
    border-radius: var(--radius-2); color: var(--text); font: inherit; font-size: 13px; padding: 9px 11px;
  }
  .in:focus { outline: none; border-color: var(--accent-line); }
  .btn {
    background: var(--surface-2); border: 1px solid var(--line-3); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; font-weight: 500;
    padding: 8px 14px; cursor: pointer; white-space: nowrap;
  }
  .btn:hover:not(:disabled) { background: var(--surface-3); }
  .btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn.primary { background: var(--accent); border-color: transparent; color: var(--accent-ink); font-weight: 600; }
  .btn.primary:hover:not(:disabled) { filter: brightness(1.08); }
  .note { margin: 0; font-size: 11.5px; color: var(--text-3); line-height: 1.5; }
  .or {
    display: flex; align-items: center; gap: 10px; color: var(--text-4); font-size: 11px; margin: 2px 0;
  }
  .or::before, .or::after { content: ""; flex: 1; height: 1px; background: var(--line-2); }
  .err {
    font-size: 12px; color: var(--bad); background: var(--bad-soft);
    border: 1px solid var(--bad-line); border-radius: var(--radius-2); padding: 7px 10px;
  }
</style>
