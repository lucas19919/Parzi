<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { tooltip } from "./tooltip";

  export let title = "";

  const dispatch = createEventDispatcher<{ close: void }>();
</script>

<div class="head">
  <div class="left">
    <h2>{title}</h2>
  </div>
  <button class="icon-btn" use:tooltip={"Close"} on:click={() => dispatch("close")}>
    <Icon name="close" />
  </button>
</div>

<style>
  .head {
    height: 42px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 calc(var(--space) * 4) 0 calc(var(--space) * 6);
    border-bottom: 1px solid var(--line);
    user-select: none;
  }
  .left {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-width: 0;
    flex: 1;
  }
  h2 {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-base);
    font-weight: 600;
    letter-spacing: -0.2px;
    color: var(--text);
  }
  .icon-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--line);
    color: var(--text);
  }
</style>
