<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import type { Question } from "./api";

  export let question: Question;

  const dispatch = createEventDispatcher<{ answer: { text: string } }>();
  const SKIPPED = "Skipped: decide yourself and say what you assumed.";

  let picked = new Set<number>();
  let other = false;
  let text = "";
  let shownKey = "";
  let field: HTMLTextAreaElement | null = null;
  $: if (question.key !== shownKey) {
    shownKey = question.key;
    picked = new Set();
    other = question.options.length === 0;
    text = "";
  }

  $: multi = !!question.multi;
  $: details = question.details ?? [];
  $: answer = [
    ...[...picked].sort((a, b) => a - b).map((i) => question.options[i]),
    ...(other && text.trim() ? [text.trim()] : []),
  ].join(", ");

  function pick(i: number) {
    if (multi) {
      picked = new Set(picked);
      if (picked.has(i)) picked.delete(i);
      else picked.add(i);
    } else {
      picked = new Set([i]);
      other = false;
    }
  }

  async function chooseOther() {
    if (multi) other = !other;
    else {
      other = true;
      picked = new Set();
    }
    await tick();
    if (other) field?.focus();
  }

  function submit() {
    if (answer) dispatch("answer", { text: answer });
  }
</script>

<div class="q" role="group" aria-label="Question">
  <div class="head t-section">{question.question}</div>
  <div class="list" role={multi ? "group" : "radiogroup"} data-ui-list>
    {#each question.options as opt, i (i)}
      <button class="choice" class:on={picked.has(i)} role={multi ? "checkbox" : "radio"} aria-checked={picked.has(i)} on:click={() => pick(i)}>
        <span class="mark" class:box={multi} />
        <span class="text">
          <span class="label">{opt}</span>
          {#if details[i]}<span class="desc">{details[i]}</span>{/if}
        </span>
      </button>
    {/each}
    <div class="choice other" class:on={other}>
      {#if question.options.length}
        <button class="other-pick" role={multi ? "checkbox" : "radio"} aria-checked={other} on:click={chooseOther}>
          <span class="mark" class:box={multi} />
          <span class="label">Other</span>
        </button>
      {/if}
      {#if other}
        <textarea
          bind:this={field}
          bind:value={text}
          rows="2"
          aria-label="Your answer"
          placeholder="Your answer… (Ctrl+Enter sends)"
          on:keydown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              submit();
            }
          }}
        />
      {/if}
    </div>
  </div>
  <div class="actions">
    <button class="btn quiet" on:click={() => dispatch("answer", { text: SKIPPED })}>Skip</button>
    <button class="btn primary" disabled={!answer} on:click={submit}>Submit</button>
  </div>
</div>

<style>
  .q {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    font-size: var(--fs-md);
    color: var(--text);
  }
  .head {
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
        border-bottom: 1px solid var(--line);
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .choice {
    display: flex;
    align-items: flex-start;
    gap: calc(var(--space) * 3);
    width: 100%;
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    background: none;
    border: none;
    border-bottom: 1px solid var(--line);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button.choice:hover,
  .other-pick:hover {
    background: var(--hover);
  }
  .choice.other {
    flex-direction: column;
    gap: calc(var(--space) * 2);
    cursor: default;
  }
  .other-pick {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    width: calc(100% + 28px);
    margin: calc(var(--space) * -3) calc(var(--space) * -4) 0;
    padding: calc(var(--space) * 3) calc(var(--space) * 4) 0;
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    min-width: 0;
  }
  .label {
    font-weight: 500;
  }
  .desc {
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }
  .mark {
    position: relative;
    width: 16px;
    height: 16px;
    flex: none;
    margin-top: var(--space);
    border: 1.5px solid var(--muted);
    border-radius: 50%;
  }
  .mark.box {
    border-radius: var(--radius-sm);
  }
  .on .mark,
  .other-pick[aria-checked="true"] .mark {
    border-color: var(--accent);
  }
  .on .mark::after,
  .other-pick[aria-checked="true"] .mark::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: inherit;
    background: var(--accent);
  }
  textarea {
    width: 100%;
    min-height: 56px;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    resize: vertical;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  textarea:focus {
    border-color: var(--accent);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
  }
  .btn {
    height: 30px;
    padding: 0 calc(var(--space) * 4);
    font-size: var(--fs-sm);
  }
  .btn.quiet {
    background: none;
    border-color: transparent;
    color: var(--muted);
  }
  .btn.quiet:hover:not(:disabled) {
    background: var(--line);
    color: var(--text);
  }
</style>
