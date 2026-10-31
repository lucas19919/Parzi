<!--
  The frame of every full page in the session area, so a title and a card sit in
  the same place on all of them. Pages fill the slots and set no positions.
  bar:     a .page-tabs holding tab buttons or one .page-title
  rail:    optional cards in a fixed column on the left
  default: cards in the centred column
-->
<div class="page">
  <nav class="page-bar"><slot name="bar" /></nav>
  <div class="scroll">
    <div class="frame" class:railed={$$slots.rail}>
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
  /* The outer tracks stay equal while there is room, so the column does not move between pages. */
  .frame.railed {
    display: grid;
    grid-template-columns: minmax(calc(var(--space) * 62), 1fr) minmax(0, 840px) minmax(0, 1fr);
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
