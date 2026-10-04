<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { faviconUrl } from "./browserData";
  import { bare, type Suggestion } from "./suggest";

  export let rows: Suggestion[] = [];
  export let active = -1;
  export let typed = "";

  const dispatch = createEventDispatcher<{ pick: { row: Suggestion }; hover: { index: number } }>();
  let broken = new Set<string>();

  function split(text: string, term: string): [string, string, string] {
    const t = term.trim().toLowerCase();
    const at = t ? text.toLowerCase().indexOf(t) : -1;
    if (at < 0) return [text, "", ""];
    return [text.slice(0, at), text.slice(at, at + t.length), text.slice(at + t.length)];
  }
</script>

<div class="list" role="listbox" aria-label="Suggestions">
  {#each rows as row, i (row.kind + row.url + row.text)}
    <button
      class="row"
      class:on={i === active}
      role="option"
      aria-selected={i === active}
      tabindex="-1"
      on:mousedown|preventDefault={() => dispatch("pick", { row })}
      on:mousemove={() => i !== active && dispatch("hover", { index: i })}
    >
      <span class="icon">
        {#if (row.kind === "history" || row.kind === "bookmark") && faviconUrl(row.url) && !broken.has(row.url)}
          <img src={faviconUrl(row.url)} alt="" on:error={() => (broken = new Set(broken).add(row.url))} />
        {:else}
          <Icon name={row.kind === "search" || row.kind === "phrase" ? "search" : row.kind === "bookmark" ? "pin" : "globe"} size={14} />
        {/if}
      </span>
      {#if row.kind === "phrase"}
        {@const [, hit, rest] = split(row.text, typed)}
        <span class="main">{#if hit && row.text.toLowerCase().startsWith(typed.trim().toLowerCase())}{hit}<b>{rest}</b>{:else}<b>{row.text}</b>{/if}</span>
      {:else if row.kind === "search"}
        <span class="main">{row.text}</span>
        <span class="sub">DuckDuckGo search</span>
      {:else if row.kind === "url"}
        <span class="main">{row.text}</span>
        <span class="sub">Open site</span>
      {:else}
        {@const [a, hit, b] = split(row.text, typed)}
        <span class="main">{a}<b>{hit}</b>{b}</span>
        <span class="sub url">{bare(row.url)}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    padding: 5px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }
  .row.on {
    background: var(--line);
  }
  .icon {
    width: 18px;
    flex: none;
    display: inline-flex;
    justify-content: center;
    color: var(--faint);
  }
  .icon img {
    width: 16px;
    height: 16px;
    border-radius: 3px;
  }
  .main {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .main b {
    font-weight: 600;
  }
  .sub {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--faint);
  }
  .sub::before {
    content: "— ";
  }
  .url {
    color: color-mix(in srgb, var(--accent) 70%, var(--muted));
  }
</style>
