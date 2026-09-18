<script lang="ts">
  // Settings shell: tab routing + shared toast. Each section mounts lazily
  // and loads only its own data, so opening settings is instant and a hung
  // provider/MCP probe can never block the other tabs.
  import GeneralSection from "./settings/GeneralSection.svelte";
  import ProvidersSection from "./settings/ProvidersSection.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import ConnectorsSection from "./settings/ConnectorsSection.svelte";
  import AgentToolsSection from "./settings/AgentToolsSection.svelte";
  import SkillsSection from "./settings/SkillsSection.svelte";
  import ContextSection from "./settings/ContextSection.svelte";
  import SystemSection from "./settings/SystemSection.svelte";

  export let settingsTab = "general";
  /** Stage mode: render one section full-bleed without modal chrome. */
  export let bareSection: string | null = null;
  /** Preselects the workspace in the Context tab. */
  export let currentProject = "default";

  // Map legacy/deep tabs to the consolidated categories
  function normalizeTab(t: string): string {
    if (["general", "application"].includes(t)) return "general";
    if (t === "appearance") return "appearance";
    if (t === "providers") return "providers";
    if (t === "connectors") return "connectors";
    if (t === "tools") return "tools";
    if (t === "skills") return "skills";
    if (t === "context") return "context";
    return "system";
  }

  let activeTab = normalizeTab(settingsTab);
  $: if (bareSection) activeTab = normalizeTab(bareSection);

  let statusMsg = "";
  function notify(msg: string) {
    statusMsg = msg;
    setTimeout(() => {
      if (statusMsg === msg) statusMsg = "";
    }, 3200);
  }
</script>

<div class="settings-root">
  <!-- Content Scroll Area (navigation lives in the sidebar).
       Sections mount only when active: their onMount fetches just their tab. -->
  <div class="settings-content-scroll">
    {#if activeTab === "general"}
      <GeneralSection {notify} />
    {:else if activeTab === "providers"}
      <ProvidersSection {notify} />
    {:else if activeTab === "appearance"}
      <AppearanceSection {notify} />
    {:else if activeTab === "connectors"}
      <ConnectorsSection {notify} />
    {:else if activeTab === "tools"}
      <AgentToolsSection {notify} />
    {:else if activeTab === "skills"}
      <SkillsSection {notify} />
    {:else if activeTab === "context"}
      <ContextSection {notify} {currentProject} />
    {:else}
      <SystemSection {notify} />
    {/if}
  </div>

  <!-- Sync Toast -->
  {#if statusMsg}
    <div class="sync-toast">
      <span class="toast-dot" /><span>{statusMsg}</span>
    </div>
  {/if}
</div>

<style>
  .settings-root {
    width: 100%;
    max-width: 860px;
    margin: 0 auto;
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative;
    user-select: none;
  }

  /* Content Scroll */
  .settings-content-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 26px 28px 40px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  /* Floating sync toast: quiet confirmation, not a call to action. */
  .sync-toast {
    position: absolute;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 13px;
    background: var(--menu);
    border: 1px solid var(--line-2);
    border-radius: 9px;
    color: var(--text);
    font-size: 12.5px;
    font-weight: 500;
    box-shadow: var(--menu-shadow);
    white-space: nowrap;
  }
  .toast-dot {
    width: 7px; height: 7px; border-radius: 50%; flex: none;
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent-glow);
  }
</style>
