<script lang="ts">
  // A plan item's state as a small mark: done, working, in review, needs
  // you, started, not started or dropped. The plan in the thread and the
  // plan in the side column draw the same marks.
  import { STATE_WORDS, type ItemState } from "./planModel";

  export let state: ItemState;
  /** Decorative next to text that already says the state. */
  export let quiet = false;
</script>

<span class="mark {state}" role={quiet ? undefined : "img"} aria-label={quiet ? undefined : STATE_WORDS[state]} aria-hidden={quiet ? "true" : undefined} />

<style>
  .mark {
    position: relative;
    width: 14px;
    height: 14px;
    flex: none;
    border: 1.5px solid var(--muted);
    border-radius: 50%;
  }
  .mark.started {
    border-style: dashed;
  }
  .mark.working {
    border-color: var(--line);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  .mark.review {
    border-color: var(--accent);
  }
  .mark.review::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: 50%;
    background: var(--accent);
  }
  .mark.needs {
    border-color: var(--warn);
    background: var(--warn);
  }
  .mark.done {
    border-color: var(--ok);
    background: var(--ok);
  }
  .mark.done::after {
    content: "";
    position: absolute;
    left: 3.5px;
    top: 1.5px;
    width: 3px;
    height: 6px;
    border: solid var(--bg);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(45deg);
  }
  .mark.dropped {
    border-color: var(--line);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mark.working {
      animation: none;
    }
  }
</style>
