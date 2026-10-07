<script lang="ts">
  import Omnibar from "./Omnibar.svelte";
  import type { ComposerMode } from "./api";

  export let which = "omnibar";

  interface Draft {
    input: string;
    model: string;
    effort: string;
    permission: string;
    mode: ComposerMode;
    attachments: string[];
  }

  function draft(mode: ComposerMode = "build"): Draft {
    return { input: "", model: "auto", effort: "medium", permission: "full", mode, attachments: [] };
  }

  let v1 = draft("build");
  let v2 = draft("build");
  let v3 = draft("build");

  const demoProject = { slug: "ui-polish", title: "UI Polish", tokens: 1200 };
</script>

<div class="preview">
  {#if which === "omnibar"}
    <section>
      <header><b>1 · Current</b><span>Baseline. Full labels, full model pill.</span></header>
      <Omnibar
        bind:input={v1.input}
        bind:model={v1.model}
        bind:effort={v1.effort}
        bind:permission={v1.permission}
        bind:mode={v1.mode}
        bind:attachments={v1.attachments}
        folder="C:\\demo\\parzi"
        branch="main"
        hero
        variant={1}
        project={demoProject}
      />
    </section>
    <section>
      <header><b>2 · Icon modes</b><span>Search / Build / Work as icons only. One row.</span></header>
      <Omnibar
        bind:input={v2.input}
        bind:model={v2.model}
        bind:effort={v2.effort}
        bind:permission={v2.permission}
        bind:mode={v2.mode}
        bind:attachments={v2.attachments}
        folder="C:\\demo\\parzi"
        branch="main"
        hero
        variant={2}
        project={demoProject}
      />
    </section>
    <section>
      <header><b>3 · Project first</b><span>Project pill on its own line above the controls.</span></header>
      <Omnibar
        bind:input={v3.input}
        bind:model={v3.model}
        bind:effort={v3.effort}
        bind:permission={v3.permission}
        bind:mode={v3.mode}
        bind:attachments={v3.attachments}
        folder="C:\\demo\\parzi"
        branch="main"
        hero
        variant={3}
        project={demoProject}
      />
    </section>
    <p class="hint">Type <span class="mono">/preview</span> in any composer to come back here. Tell Lucas which number wins and what to change.</p>
  {:else}
    <p class="hint">No preview named "{which}" yet.</p>
  {/if}
</div>

<style>
  .preview {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding: 28px 24px 40px;
    align-items: center;
  }
  section {
    width: min(720px, 100%);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 0 4px;
  }
  header b {
    font-size: 13px;
    color: var(--text);
  }
  header span {
    font-size: 12px;
    color: var(--faint);
  }
  .hint {
    font-size: 12px;
    color: var(--faint);
  }
  .mono {
    font-family: var(--mono);
    color: var(--muted);
  }
</style>
