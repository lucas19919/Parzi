<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { tooltip } from "./tooltip";

  export let title = "";

  const dispatch = createEventDispatcher<{ close: void }>();
</script>

<div class="head page-bar">
  <div class="left">
    <h2>{title}</h2>
  </div>
  <button class="back-btn" use:tooltip={"Close"} on:click={() => dispatch("close")}>
    <Icon name="close" />
  </button>
</div>

<style>
  .head {
    justify-content: space-between;
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
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
  }
</style>
