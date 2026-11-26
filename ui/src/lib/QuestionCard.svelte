<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { Proposal, Question } from "./api";
  import { mdHtml, richReady } from "./md";
  import { LINE_WORDS, questionParts } from "./questionText";

  /** A question from a tool call, answered with a choice or your own words. */
  export let question: Question | null = null;
  /** Or a plan the agent proposed (the session's proposal): Accept or request changes. */
  export let proposal: Proposal | null = null;

  const dispatch = createEventDispatcher<{ answer: { text: string }; accept: void; revise: void }>();
  const SKIPPED = "Skipped: decide yourself and say what you assumed.";

  let picked = new Set<number>();
  let text = "";
  let shownKey = "";
  let field: HTMLTextAreaElement | null = null;

  let proposalKey = "";
  let openSteps = new Set<number>();

  $: if (question && question.key !== shownKey) {
    shownKey = question.key;
    picked = new Set();
    text = "";
    if (!question.options.length) void tick().then(() => field?.focus());
  }

  $: nextProposalKey = proposal ? `${proposal.doc ?? ""}\n${proposal.summary ?? ""}\n${proposal.items.map((s) => s.title).join("|")}` : "";
  $: if (proposal && nextProposalKey !== proposalKey) {
    proposalKey = nextProposalKey;
    const detailChars = proposal.items.reduce((acc, s) => acc + (s.detail?.length ?? 0), 0);
    // Keep long multi-step plans scannable by default; expand short plans automatically.
    openSteps = proposal.items.length <= 2 && detailChars <= 240
      ? new Set(proposal.items.map((_, i) => i))
      : new Set();
  }

  $: hasDetails = !!proposal?.items.some((s) => !!s.detail?.trim());
  $: allDetailsOpen = !!proposal && proposal.items.filter((s) => !!s.detail?.trim()).every((_, idx, arr) => {
    void idx;
    void arr;
    return proposal!.items.every((s, i) => !s.detail?.trim() || openSteps.has(i));
  });

  function toggleStep(i: number) {
    const next = new Set(openSteps);
    if (next.has(i)) next.delete(i);
    else next.add(i);
    openSteps = next;
  }

  function toggleAllSteps() {
    if (!proposal) return;
    if (allDetailsOpen) openSteps = new Set();
    else openSteps = new Set(proposal.items.map((s, i) => (s.detail?.trim() ? i : -1)).filter((i) => i >= 0));
  }

  $: parts = questionParts(question?.question ?? "");
  const openDoc = (path: string) => document.dispatchEvent(new CustomEvent("parzi:open-file", { detail: path }));
  $: options = question?.options ?? [];
  $: multi = !!question?.multi;
  $: details = question?.details ?? [];
  $: chosen = [...picked].sort((a, b) => a - b).map((i) => options[i]);
  $: answer = [...chosen, ...(text.trim() ? [text.trim()] : [])].join(", ");

  function send(value: string) {
    if (value.trim()) dispatch("answer", { text: value.trim() });
  }

  /** One choice answers at once; with several, a click toggles. */
  function pick(i: number) {
    if (!multi) return send(options[i]);
    picked = new Set(picked);
    if (picked.has(i)) picked.delete(i);
    else picked.add(i);
  }

  function sendText() {
    if (multi) send(answer);
    else send(text);
  }

  function grow(el: HTMLTextAreaElement) {
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (!question || !options.length || e.metaKey || e.ctrlKey || e.altKey) return;
    const tag = (e.target as HTMLElement | null)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || (e.target as HTMLElement | null)?.isContentEditable) return;
    if (e.key >= "1" && e.key <= "9") {
      const idx = Number(e.key) - 1;
      if (idx < options.length) {
        e.preventDefault();
        pick(idx);
      }
    }
  }
</script>

<svelte:window on:keydown={onWindowKeydown} />

{#if proposal}
  <div class="q plan" role="group" aria-label="Proposed plan">
    <div class="head">
      <div class="head-main">
        <span class="badge-icon" aria-hidden="true"><Icon name="plan" size={14} /></span>
        <span class="title">Proposed plan</span>
        <span class="count">{proposal.items.length} {proposal.items.length === 1 ? "step" : "steps"}</span>
      </div>
      <div class="head-actions">
        {#if hasDetails}
          <button class="ghost-btn" on:click={toggleAllSteps}>
            {allDetailsOpen ? "Hide details" : "Show details"}
          </button>
        {/if}
        {#if proposal.doc}
          <button class="ghost-btn" on:click={() => proposal?.doc && openDoc(proposal.doc)}>
            Open full plan
          </button>
        {/if}
      </div>
    </div>

    {#if proposal.summary}
      <div class="summary msg">{@html mdHtml(proposal.summary, $richReady)}</div>
    {/if}

    <ol class="steps">
      {#each proposal.items as step, i (i)}
        {@const hasDetail = !!step.detail?.trim()}
        {@const open = hasDetail && openSteps.has(i)}
        <li class="step-row" class:open>
          {#if hasDetail}
            <button class="step-head" aria-expanded={open} on:click={() => toggleStep(i)}>
              <span class="num">{i + 1}</span>
              <span class="step-title">{step.title}</span>
              <span class="chev" class:open aria-hidden="true"><Icon name="chevDown" size={12} /></span>
            </button>
            {#if open && step.detail}
              <div class="step-detail msg">{@html mdHtml(step.detail, $richReady)}</div>
            {/if}
          {:else}
            <div class="step-head plain">
              <span class="num">{i + 1}</span>
              <span class="step-title">{step.title}</span>
            </div>
          {/if}
        </li>
      {/each}
    </ol>

    <div class="accept-row">
      <button class="btn" on:click={() => dispatch("revise")}>Request changes</button>
      <button class="btn primary accept" on:click={() => dispatch("accept")}>Accept plan</button>
    </div>
  </div>
{:else if question}
  <div class="q" role="group" aria-label="Question">
    <div class="head">
      <div class="title msg">{@html mdHtml(parts.head, $richReady)}</div>
      <button class="ghost-btn" on:click={() => dispatch("answer", { text: SKIPPED })}>Skip</button>
    </div>

    {#if parts.lines.length}
      <ul class="about" aria-label="What you are answering about">
        {#each parts.lines as line, i (i)}
          {#if line.kind === "heading"}
            <li class="group msg">{@html mdHtml(line.text, $richReady)}</li>
          {:else if line.kind === "item"}
            <li class="about-item">
              {#if line.mark}<span class="mark">{line.mark}</span>{/if}
              <span class="msg">{@html mdHtml(line.text, $richReady)}</span>
            </li>
          {:else}
            <li class="change {line.kind}">
              <span class="sign" aria-label={LINE_WORDS[line.kind]}>{line.kind === "add" ? "+" : line.kind === "drop" ? "−" : "~"}</span>
              <span class:struck={line.kind === "drop"}>{line.text}</span>
            </li>
          {/if}
        {/each}
      </ul>
    {/if}

    <div class="tiles" role={multi ? "group" : "list"} data-ui-list>
      {#each options as opt, i (i)}
        {@const on = picked.has(i)}
        <button
          class="tile"
          class:on
          role={multi ? "checkbox" : undefined}
          aria-checked={multi ? on : undefined}
          on:click={() => pick(i)}
        >
          <span class="indicator" class:box={multi} class:on aria-hidden="true">
            {#if on}<Icon name="check" size={12} />{/if}
          </span>
          <span class="text">
            <span class="label">{opt}</span>
            {#if details[i]}<span class="desc">{details[i]}</span>{/if}
          </span>
          {#if options.length <= 9}
            <kbd class="key">{i + 1}</kbd>
          {/if}
        </button>
      {/each}

      <div class="tile own" class:filled={!!text.trim()}>
        <textarea
          bind:this={field}
          bind:value={text}
          rows="1"
          aria-label="Your answer"
          placeholder={options.length ? "Or write your own answer…" : "Your answer…"}
          on:input={(e) => grow(e.currentTarget)}
          on:keydown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendText();
            }
          }}
        />
        {#if !multi}
          <button class="send" aria-label="Send" disabled={!text.trim()} on:click={sendText}>
            <Icon name="arrowUp" size={14} />
          </button>
        {/if}
      </div>
    </div>

    {#if multi}
      <div class="foot">
        <button class="btn primary" disabled={!answer} on:click={() => send(answer)}>
          {chosen.length ? `Send (${chosen.length})` : "Send"}
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .q {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3.5);
    padding: calc(var(--space) * 4);
    background: var(--surface);
    border: 1px solid var(--edge);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
    font-size: var(--fs-md);
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(var(--space) * 3);
  }
  .head-main {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    flex: none;
  }
  .badge-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    flex: none;
  }
  .title {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-base);
    font-weight: 600;
    line-height: 1.45;
  }
  .title :global(p) {
    margin: 0;
  }
  .count {
    flex: none;
    padding: calc(var(--space) * 0.5) calc(var(--space) * 2);
    background: var(--fill);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 500;
  }
  .ghost-btn {
    flex: none;
    padding: calc(var(--space) * 1) calc(var(--space) * 2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    transition: background var(--dur) ease, color var(--dur) ease, border-color var(--dur) ease;
  }
  .ghost-btn:hover {
    background: var(--fill);
    border-color: var(--edge);
    color: var(--text);
  }
  .about {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 1.5);
    max-height: 240px;
    margin: 0;
    padding: calc(var(--space) * 2.5) calc(var(--space) * 3);
    overflow-y: auto;
    list-style: none;
    background: var(--fill);
    border: 1px solid var(--edge);
    border-radius: var(--radius-in);
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.55;
  }
  .about :global(p) {
    margin: 0;
  }
  .about .group {
    margin-top: calc(var(--space) * 1.5);
    color: var(--text);
    font-weight: 600;
  }
  .about .group:first-child {
    margin-top: 0;
  }
  .about-item {
    display: flex;
    gap: calc(var(--space) * 2);
    align-items: baseline;
  }
  .mark {
    flex: none;
    color: var(--faint);
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }
  .change {
    display: flex;
    gap: calc(var(--space) * 2);
  }
  .sign {
    flex: none;
    width: calc(var(--space) * 3);
    font-family: var(--mono);
    text-align: center;
  }
  .add .sign {
    color: var(--ok-text);
  }
  .drop .sign {
    color: var(--bad-text);
  }
  .struck {
    text-decoration: line-through;
  }
  .tiles {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 1.5);
  }
  .tile {
    display: flex;
    align-items: flex-start;
    gap: calc(var(--space) * 3);
    width: 100%;
    min-height: var(--h-row);
    padding: calc(var(--space) * 2.5) calc(var(--space) * 3);
    background: var(--fill);
    border: 1px solid var(--edge);
    border-radius: var(--radius-in);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--dur) ease, border-color var(--dur) ease;
  }
  button.tile:hover {
    background: var(--fill-hover);
    border-color: var(--line);
  }
  .tile.on {
    background: var(--selected);
    border-color: var(--accent);
  }
  .indicator {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: calc(var(--space) * 0.5);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1.5px solid var(--faint);
    border-radius: 50%;
    color: var(--bg);
    transition: background var(--dur) ease, border-color var(--dur) ease;
  }
  .indicator.box {
    border-radius: var(--radius-sm);
  }
  button.tile:hover .indicator {
    border-color: var(--muted);
  }
  .indicator.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .label {
    font-weight: 500;
    line-height: 1.4;
  }
  .desc {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .key {
    flex: none;
    align-self: center;
    min-width: 20px;
    padding: calc(var(--space) * 0.5) calc(var(--space) * 1.5);
    background: var(--surface);
    border: 1px solid var(--edge);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    text-align: center;
  }
  .own {
    align-items: center;
    cursor: text;
    background: transparent;
    border-color: var(--edge);
  }
  .own:focus-within,
  .own.filled {
    background: var(--fill);
    border-color: var(--line);
  }
  textarea {
    flex: 1;
    min-width: 0;
    padding: 0;
    resize: none;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font: inherit;
    line-height: 1.45;
  }
  textarea::placeholder {
    color: var(--muted);
  }
  .send {
    flex: none;
    width: var(--h-sm);
    height: var(--h-sm);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--accent);
    border: none;
    border-radius: 50%;
    color: var(--bg);
    cursor: pointer;
  }
  .send:disabled {
    background: var(--fill);
    color: var(--muted);
    cursor: default;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
  }
  /* Proposed plan */
  .summary {
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1.6;
  }
  .summary :global(p:first-child) {
    margin-top: 0;
  }
  .summary :global(p:last-child) {
    margin-bottom: 0;
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 1.5);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .step-row {
    background: var(--fill);
    border: 1px solid var(--edge);
    border-radius: var(--radius-in);
    overflow: hidden;
    transition: border-color var(--dur) ease;
  }
  .step-row:hover {
    border-color: var(--line);
  }
  .step-head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2.5);
    width: 100%;
    padding: calc(var(--space) * 2.5) calc(var(--space) * 3);
    background: transparent;
    border: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .step-head.plain {
    cursor: default;
  }
  .num {
    flex: none;
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--surface);
    border: 1px solid var(--edge);
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    font-weight: 500;
  }
  .step-title {
    flex: 1;
    min-width: 0;
    font-weight: 500;
    line-height: 1.45;
  }
  .chev {
    flex: none;
    display: inline-flex;
    color: var(--muted);
    transition: transform var(--dur) ease;
  }
  .chev.open {
    transform: rotate(180deg);
  }
  .step-detail {
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 2.5) calc(var(--space) * 10);
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.55;
  }
  .step-detail :global(p:first-child) {
    margin-top: 0;
  }
  .step-detail :global(p:last-child) {
    margin-bottom: 0;
  }
  .accept-row {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
    padding-top: var(--space);
  }
  .accept-row .btn,
  .foot .btn {
    height: var(--h);
    padding: 0 calc(var(--space) * 4);
    font-size: var(--fs-sm);
  }
</style>
