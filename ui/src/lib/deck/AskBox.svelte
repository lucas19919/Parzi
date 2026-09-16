<script lang="ts">
  import { createEventDispatcher } from "svelte";

  /** Off = the header (cheap, reads state). On = the orchestrator (changes the plan). */
  export let direct = false;
  export let busy = false;
  export let headerModel = "";
  export let orchestratorModel = "";
  /** Set while no orchestrator session can be reached (Direct stays off). */
  export let canDirect = true;

  const dispatch = createEventDispatcher<{
    ask: { prompt: string; to: "header" | "orchestrator" };
  }>();

  let prompt = "";
  let box: HTMLTextAreaElement | null = null;

  const short = (spec: string) => {
    if (!spec) return "auto";
    const bit = spec.split("/").pop() ?? spec;
    return bit.replace(/-thinking$/i, "");
  };

  function send() {
    const text = prompt.trim();
    if (!text || busy) return;
    dispatch("ask", { prompt: text, to: direct ? "orchestrator" : "header" });
    prompt = "";
    if (box) box.style.height = "auto";
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  function grow() {
    if (!box) return;
    box.style.height = "auto";
    box.style.height = `${Math.min(box.scrollHeight, 160)}px`;
  }
</script>

<div class="ask" class:direct>
  <textarea
    bind:this={box}
    bind:value={prompt}
    class="in"
    rows="1"
    placeholder={direct ? "Change the plan…" : "What are we building?"}
    on:input={grow}
    on:keydown={onKey}
  ></textarea>
  <div class="row">
    <div class="who" role="group" aria-label="Who to ask">
      <button class="who-btn" class:on={!direct} on:click={() => (direct = false)}>Header</button>
      <button
        class="who-btn"
        class:on={direct}
        disabled={!canDirect}
        title={canDirect ? "Change the plan" : "No orchestrator session yet"}
        on:click={() => (direct = true)}
      >Plan</button>
    </div>
    <span class="to">{short(direct ? orchestratorModel : headerModel)}</span>
    <span class="spacer" />
    <button
      class="send"
      disabled={busy || !prompt.trim()}
      on:click={send}
    >{busy ? "…" : "Ask"}</button>
  </div>
</div>

<style>
  .ask {
    background: var(--panel); border: 1px solid var(--line-2); border-radius: var(--glass-radius);
    padding: 12px 14px 10px; display: flex; flex-direction: column; gap: 8px;
  }
  .ask.direct { border-color: var(--accent-line); }
  .in {
    width: 100%; box-sizing: border-box; resize: none; background: transparent; border: none;
    color: var(--text); font: inherit; font-size: 14px; line-height: 1.5; outline: none; padding: 2px 0;
  }
  .in::placeholder { color: var(--text-4); }
  .row { display: flex; align-items: center; gap: 8px; }
  .who {
    display: inline-flex; background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-pill); padding: 1px;
  }
  .who-btn {
    background: none; border: none; border-radius: var(--radius-pill);
    color: var(--text-3); font: inherit; font-size: 11px; padding: 2px 9px; cursor: pointer;
  }
  .who-btn:hover:not(:disabled) { color: var(--text); }
  .who-btn.on { background: var(--surface-3); color: var(--text); }
  .who-btn:disabled { opacity: 0.4; cursor: default; }
  .to { font-size: 11px; color: var(--text-4); }
  .spacer { flex: 1; }
  .send {
    background: var(--accent); border: none; color: var(--accent-ink);
    font: inherit; font-size: 12px; font-weight: 600; padding: 4px 12px;
    border-radius: var(--radius-pill); cursor: pointer;
  }
  .send:hover:not(:disabled) { filter: brightness(1.08); }
  .send:disabled { opacity: 0.35; cursor: default; }
</style>
