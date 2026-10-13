<script lang="ts">
  // A plan item up close, shown while the pointer rests on it: what it is,
  // who works it, how the last run went, its PR. Read only. It lives on
  // the body so it can leave the panel, and hides a web page's native view
  // while it shows (that view is drawn above all HTML).
  import Icon from "./Icon.svelte";
  import { setOverlay } from "./overlay";
  import { ago, basename, clip, formatCost, formatElapsed, plainText, runWord } from "./format";
  import { cardSpot, sessionTone, STATE_WORDS, TONE_WORDS, type ItemView } from "./planModel";

  export let view: ItemView;
  export let anchor: HTMLElement;
  export let id = "";
  export let running: Set<string> = new Set();
  export let waiting: { has(id: string): boolean } = new Set<string>();

  const SESSIONS = 4;
  const FILES = 4;
  const CHECKS = 4;

  let seq = 0;
  let spot = "visibility:hidden;";

  $: item = view.item;
  $: sessions = view.sessions.slice(0, SESSIONS);
  $: last = view.last?.run ?? null;
  $: files = last?.files.slice(0, FILES) ?? [];
  $: moreFiles = last ? Math.max(0, last.files.length - FILES) + last.files_more : 0;
  $: checks = last?.checks.slice(0, CHECKS) ?? [];
  // "Finished 55m ago · 5m · $0.31"
  $: runMeta = last ? [`${runWord(last.end)} ${ago(last.ended)}`, last.ms ? formatElapsed(last.ms) : "", formatCost(last.cost_usd)].filter(Boolean).join(" · ") : "";
  $: pr = view.pr;
  $: prWords = pr ? [pr.state === "merged" ? "Merged" : pr.draft ? "Draft" : "Open", CHECK_WORDS[pr.checks]].filter(Boolean).join(" · ") : "";

  const CHECK_WORDS: Record<string, string> = { pass: "checks pass", fail: "checks fail", pending: "checks running", none: "" };

  // Body-mounted and placed once it has a size; re-placed when its content
  // (and so its height) changes.
  function float(node: HTMLElement, _key: unknown) {
    const key = `plan-card-${++seq}-${Math.random().toString(36).slice(2, 6)}`;
    document.body.appendChild(node);
    setOverlay(key, true);
    const place = () => {
      const r = anchor.getBoundingClientRect();
      const s = cardSpot(r, { width: node.offsetWidth, height: node.offsetHeight }, { width: window.innerWidth, height: window.innerHeight });
      spot = `left:${s.left}px;top:${s.top}px;`;
    };
    place();
    return {
      update: place,
      destroy() {
        setOverlay(key, false);
        node.remove();
      },
    };
  }
</script>

<div class="plan-card" role="tooltip" {id} style={spot} use:float={view}>
  <p class="state"><i class="dot s-{view.state}" />{STATE_WORDS[view.state]}{#if item.phase}<span class="phase"> · {item.phase}</span>{/if}</p>
  <p class="title" class:struck={view.state === "dropped"}>{item.title}</p>
  {#if item.detail}<p class="detail">{item.detail}</p>{/if}
  {#if view.reason}<p class="why">{view.reason}</p>{/if}
  {#if view.after.length}<p class="quiet">After {view.after.join(", ")}</p>{/if}
  {#if item.note}<p class="quiet">Note: {item.note}</p>{/if}

  {#if sessions.length}
    <section>
      <h4>Sessions</h4>
      <ul class="list">
        {#each sessions as s (s.id)}
          {@const tone = sessionTone(s, running, waiting)}
          <li><i class="dot t-{tone}" /><span class="grow">{clip(s.title || "Untitled session", 48)}</span><span class="quiet">{TONE_WORDS[tone]}</span></li>
        {/each}
        {#if view.sessions.length > SESSIONS}<li class="quiet more">+{view.sessions.length - SESSIONS} more</li>{/if}
      </ul>
    </section>
  {/if}

  {#if last}
    <section>
      <h4>Last run · {runMeta}</h4>
      {#if last.summary}<p class="summary">{clip(plainText(last.summary), 240)}</p>{/if}
      {#if files.length}
        <ul class="list">
          {#each files as f (f.path)}
            <li><Icon name="file" size={12} /><span class="mono grow">{basename(f.path)}</span></li>
          {/each}
          {#if moreFiles}<li class="quiet more">+{moreFiles} more</li>{/if}
        </ul>
      {/if}
      {#if checks.length}
        <ul class="list">
          {#each checks as c, i (i)}
            <li class:ok={c.ok} class="check"><Icon name={c.ok ? "checkCircle" : "xCircle"} size={12} /><span class="mono grow">{clip(c.cmd, 40)}</span><span class="quiet">{c.ok ? "Passed" : "Failed"}</span></li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}

  {#if pr}
    <section>
      <h4>Pull request #{pr.number}</h4>
      <p class="detail">{pr.title}</p>
      {#if prWords}<p class="quiet">{prWords}</p>{/if}
    </section>
  {/if}

  <p class="hint">Click to quote it in the composer</p>
</div>

<style>
  .plan-card {
    position: fixed;
    z-index: 650;
    width: 300px;
    max-height: calc(100vh - 16px);
    overflow: hidden;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1.45;
    pointer-events: none;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .title {
    font-size: var(--fs-base);
    font-weight: 600;
  }
  .struck {
    text-decoration: line-through;
    color: var(--muted);
  }
  .detail {
    color: var(--text);
  }
  .why {
    font-weight: 600;
  }
  .quiet {
    color: var(--muted);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }
  h4 {
    margin: 0;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .summary {
    font-size: var(--fs-sm);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-sm);
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 20px;
    color: var(--muted);
  }
  .list .grow {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .more {
    padding-left: 20px;
  }
  .mono {
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-xs);
  }
  .check {
    color: var(--bad);
  }
  .check.ok {
    color: var(--ok);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint);
  }
  .s-done,
  .t-ok {
    background: var(--muted);
  }
  .s-needs,
  .t-need {
    background: var(--warn);
  }
  .s-working,
  .t-live {
    background: var(--ok);
  }
  .s-review {
    background: transparent;
    box-shadow: inset 0 0 0 2px var(--ok);
  }
  .s-started {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--muted);
  }
  .s-todo,
  .s-dropped {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--faint);
  }
  .t-bad {
    background: var(--bad);
  }
  .hint {
    padding-top: 2px;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
</style>
