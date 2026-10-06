<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";

  export let title = "";

  const dispatch = createEventDispatcher<{ close: void }>();
</script>

<div class="head">
  <div class="left">
    <h2>{title}</h2>
  </div>
  <button class="icon-btn" title="Close" on:click={() => dispatch("close")}>
    <Icon name="close" size={13} stroke={2} />
  </button>
</div>

<style>
  .head {
    height: 42px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--line);
    user-select: none;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }
  h2 {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14.5px;
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
    border-radius: 5px;
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--line);
    color: var(--text);
  }
</style>
