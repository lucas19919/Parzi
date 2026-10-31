<!--
  The frame of every full page in the session area, so a title and a card sit in
  the same place on all of them. Pages fill the slots and set no positions.
  bar:     a .page-tabs holding tab buttons or one .page-title
  tools:   instead of bar: controls in the bar, lined up with the column
  rail:    optional cards in a fixed column on the left, level with the first card
  default: cards in the centred column
-->
<div class="page" class:railed={$$slots.rail}>
  {#if $$slots.tools}
    <nav class="page-bar tracks"><div class="tools"><slot name="tools" /></div></nav>
  {:else}
    <nav class="page-bar"><slot name="bar" /></nav>
  {/if}
  <div class="scroll">
    <div class="frame">
      {#if $$slots.rail}<aside class="rail"><slot name="rail" /></aside>{/if}
      <div class="column"><slot /></div>
    </div>
  </div>
</div>

<style>
  .page {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* The same height as a bar of tabs: 8px + 36px + 8px. */
  .tracks {
    display: grid;
    grid-template-columns: var(--tracks);
    gap: 0;
    min-height: calc(var(--space) * 13);
    padding-inline: 0;
  }
  .tools {
    grid-column: 2;
    display: flex;
    align-items: stretch;
    gap: calc(var(--space) * 2);
    min-width: 0;
    padding: 0 calc(var(--space) * 8);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 6);
    width: 100%;
    min-width: 0;
    max-width: 840px;
    margin: 0 auto;
    padding: calc(var(--space) * 6) calc(var(--space) * 8) calc(var(--space) * 8);
  }
  /* The outer tracks stay equal while there is room, so the column does not move
     between pages. The right one keeps the bar's controls clear of the back arrow. */
  .railed {
    --tracks: minmax(calc(var(--space) * 62), 1fr) minmax(0, 840px) minmax(calc(var(--space) * 5), 1fr);
  }
  .railed .frame {
    display: grid;
    grid-template-columns: var(--tracks);
    align-items: start;
  }
  /* Its left edge is the bar's text edge: the bar's 16px inset plus a tab's 12px. */
  .rail {
    position: sticky;
    top: 0;
    width: calc(var(--space) * 55);
    margin-left: calc(var(--space) * 7);
    padding-top: calc(var(--space) * 6);
  }
</style>
