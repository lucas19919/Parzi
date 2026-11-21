<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { Proposal, Question } from "./api";
  import { LINE_WORDS, questionParts } from "./questionText";

  /** A question from a tool call, answered with a choice or your own words. */
  export let question: Question | null = null;
  /** Or a plan the agent proposed (the session's proposal): one Accept;
   *  changes are typed in the composer, which declines it (App.svelte). */
  export let proposal: Proposal | null = null;

  const dispatch = createEventDispatcher<{ answer: { text: string }; accept: void }>();
  const SKIPPED = "Skipped: decide yourself and say what you assumed.";

  let picked = new Set<number>();
  let text = "";
  let shownKey = "";
  let field: HTMLTextAreaElement | null = null;
  $: if (question && question.key !== shownKey) {
    shownKey = question.key;
    picked = new Set();
    text = "";
    if (!question.options.length) void tick().then(() => field?.focus());
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
</script>

<!-- A question as a calm card: what is asked, what it is about, and the
     answers as tiles. One choice answers with a click; several toggle and
     send together; "Something else" is always there for your own words. -->
{#if proposal}
<div class="q plan" role="group" aria-label="Proposed plan">
  <div class="head">
    <span class="title">Proposed plan</span>
    {#if proposal.doc}<button class="skip" on:click={() => proposal?.doc && openDoc(proposal.doc)}>Open full plan</button>{/if}
  </div>
  {#if proposal.summary}<p class="summary">{proposal.summary}</p>{/if}
  <ul class="steps">
    {#each proposal.items as step, i (i)}
      <li>
        <span class="ring" aria-hidden="true" />
        <span class="step">
          <span class="step-title">{step.title}</span>
          {#if step.detail}<span class="step-detail">{step.detail}</span>{/if}
        </span>
      </li>
    {/each}
  </ul>
  <div class="accept-row">
    <button class="btn primary accept" on:click={() => dispatch("accept")}>Accept</button>
  </div>
</div>
{:else if question}
<div class="q" role="group" aria-label="Question">
  <div class="head">
    <span class="title">{parts.head}</span>
    <button class="skip" on:click={() => dispatch("answer", { text: SKIPPED })}>Skip</button>
  </div>
  {#if parts.lines.length}
    <ul class="about" aria-label="What you are answering about">
      {#each parts.lines as line, i (i)}
        {#if line.kind === "heading"}
          <li class="group">{line.text}</li>
        {:else if line.kind === "item"}
          <li>{line.text}</li>
        {:else}
          <li class="change {line.kind}"><span class="sign" aria-label={LINE_WORDS[line.kind]}>{line.kind === "add" ? "+" : line.kind === "drop" ? "−" : "~"}</span><span class:struck={line.kind === "drop"}>{line.text}</span></li>
        {/if}
      {/each}
    </ul>
  {/if}
  <div class="tiles" role={multi ? "group" : "list"} data-ui-list>
    {#each options as opt, i (i)}
      <button
        class="tile"
        class:on={picked.has(i)}
        role={multi ? "checkbox" : undefined}
        aria-checked={multi ? picked.has(i) : undefined}
        on:click={() => pick(i)}
      >
        <span class="text">
          <span class="label">{opt}</span>
          {#if details[i]}<span class="desc">{details[i]}</span>{/if}
        </span>
        <span class="go" aria-hidden="true"><Icon name={multi ? "check" : "arrowRight"} size={14} /></span>
      </button>
    {/each}
    <div class="tile own" class:filled={!!text.trim()}>
      <textarea
        bind:this={field}
        bind:value={text}
        rows="1"
        aria-label="Your answer"
        placeholder={options.length ? "Something else…" : "Your answer…"}
        on:input={(e) => grow(e.currentTarget)}
        on:keydown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            sendText();
          }
        }}
      />
      {#if !multi}
        <button class="send" aria-label="Send" disabled={!text.trim()} on:click={sendText}><Icon name="arrowUp" size={14} /></button>
      {/if}
    </div>
  </div>
  {#if multi}
    <div class="foot">
      <button class="btn primary" disabled={!answer} on:click={() => send(answer)}>Send</button>
    </div>
  {/if}
</div>
{/if}

<style>
  .q {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    padding: var(--space);
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
    align-items: baseline;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 3) 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    font-weight: 600;
    line-height: 1.4;
  }
  .skip {
    flex: none;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .skip:hover {
    color: var(--text);
  }
  .about {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) / 2);
    max-height: 240px;
    margin: 0;
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 2);
    overflow-y: auto;
    list-style: none;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .about .group {
    margin-top: calc(var(--space) * 2);
    color: var(--text);
    font-weight: 600;
  }
  .about .group:first-child {
    margin-top: 0;
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
    gap: var(--space);
  }
  .tile {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    width: 100%;
    min-height: var(--h-row);
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: var(--fill);
    border: 1px solid transparent;
    border-radius: var(--radius-in);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--dur) ease;
  }
  button.tile:hover {
    background: var(--fill-hover);
  }
  .tile.on {
    background: var(--selected);
    border-color: var(--edge);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) / 2);
  }
  .label {
    font-weight: 500;
  }
  .desc {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.45;
  }
  .go {
    flex: none;
    display: inline-flex;
    color: var(--muted);
    opacity: 0;
    transition: opacity var(--dur) ease;
  }
  button.tile:hover .go,
  .tile.on .go {
    opacity: 1;
  }
  .tile.on .go {
    color: var(--text);
  }
  .own {
    cursor: text;
    background: transparent;
    border-color: var(--edge);
  }
  .own:focus-within,
  .own.filled {
    background: var(--fill);
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
    padding: var(--space) var(--space) var(--space);
  }
  /* A proposed plan: read at full size, one Accept. */
  .summary {
    margin: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3) 0;
    font-size: var(--fs-base);
    line-height: 1.55;
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2.5);
    margin: 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 3) calc(var(--space) * 2);
    list-style: none;
  }
  .steps li {
    display: flex;
    gap: calc(var(--space) * 3);
  }
  .ring {
    flex: none;
    width: 12px;
    height: 12px;
    margin-top: var(--space);
    box-sizing: border-box;
    border: 1.5px solid var(--faint);
    border-radius: 50%;
  }
  .step {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) / 2);
    min-width: 0;
  }
  .step-title {
    font-weight: 500;
    line-height: 1.45;
  }
  .step-detail {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .accept-row {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 2) calc(var(--space) * 2) calc(var(--space) * 2) calc(var(--space) * 3);
  }
  .accept {
    flex: none;
    height: var(--h);
    padding: 0 calc(var(--space) * 4);
    font-size: var(--fs-sm);
  }
  .foot .btn {
    height: var(--h);
    padding: 0 calc(var(--space) * 4);
    font-size: var(--fs-sm);
  }
</style>
