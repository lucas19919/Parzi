<script lang="ts">
  import { onMount } from "svelte";
  import { api, type PluginView, type SkillCommand } from "../api";
  import Switch from "./Switch.svelte";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  const I = {
    plus: "M12 5v14M5 12h14",
    edit: "M17 3a2.8 2.8 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    trash: "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
  };

  let plugins: PluginView[] | null = null;
  let err = "";
  let busy: Record<string, boolean> = {};

  let newSkill = "";
  let createErr = "";
  let creating = false;

  /** Expanded skill → its editable command list (null while loading). */
  let openSkill: string | null = null;
  let commands: SkillCommand[] | null = null;
  let cmdErr = "";
  let saving = false;
  /** Inline new-command draft per open skill. */
  let nc = { name: "", description: "", prompt: "" };

  onMount(async () => {
    try {
      plugins = await api.listPlugins();
    } catch (e) {
      err = String(e);
    }
  });

  /** Installed packs grouped by kind: command packs are skills. */
  $: skills = (plugins ?? []).filter((p) => p.kind === "commands");
  $: themes = (plugins ?? []).filter((p) => p.kind === "theme");
  $: packs = (plugins ?? []).filter((p) => p.kind === "mcp-pack");
  $: other = (plugins ?? []).filter((p) => !["commands", "theme", "mcp-pack"].includes(p.kind));

  async function refresh() {
    try {
      plugins = await api.listPlugins();
    } catch (e) {
      notify(`Refresh failed: ${e}`);
    }
  }

  async function create() {
    createErr = "";
    const name = newSkill.trim();
    if (!name) return;
    creating = true;
    try {
      await api.createSkill(name);
      newSkill = "";
      await refresh();
      notify(`Skill "${name}" created — add commands below`);
      void openCommands(name);
    } catch (e) {
      createErr = String(e);
    } finally {
      creating = false;
    }
  }

  async function flip(p: PluginView) {
    if (busy[p.name]) return;
    busy = { ...busy, [p.name]: true };
    try {
      await api.togglePlugin(p.name, !p.enabled);
      p.enabled = !p.enabled;
      plugins = [...(plugins ?? [])];
      notify(p.enabled ? `${p.name} enabled` : `${p.name} disabled`);
    } catch (e) {
      notify(`Toggle failed: ${e}`);
    } finally {
      const next = { ...busy };
      delete next[p.name];
      busy = next;
    }
  }

  async function openCommands(name: string) {
    if (openSkill === name) {
      openSkill = null;
      return;
    }
    openSkill = name;
    commands = null;
    cmdErr = "";
    nc = { name: "", description: "", prompt: "" };
    try {
      commands = await api.skillCommands(name);
    } catch (e) {
      cmdErr = String(e);
    }
  }

  function addCommand() {
    if (!commands) return;
    const name = nc.name.trim().toLowerCase();
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(name)) {
      cmdErr = "Command name: lowercase letters, numbers, dashes.";
      return;
    }
    if (!nc.description.trim() || !nc.prompt.trim()) {
      cmdErr = "Description and prompt are required.";
      return;
    }
    if (commands.some((c) => c.name === name)) {
      cmdErr = "That command already exists in this skill.";
      return;
    }
    cmdErr = "";
    commands = [...commands, { name, description: nc.description.trim(), prompt: nc.prompt.trim() }];
    nc = { name: "", description: "", prompt: "" };
  }

  function removeCommand(name: string) {
    if (!commands) return;
    commands = commands.filter((c) => c.name !== name);
  }

  async function saveCommands() {
    if (!openSkill || !commands) return;
    saving = true;
    cmdErr = "";
    try {
      await api.saveSkillCommands(openSkill, commands);
      notify(`Saved ${commands.length} command${commands.length === 1 ? "" : "s"} to ${openSkill}`);
    } catch (e) {
      cmdErr = String(e);
    } finally {
      saving = false;
    }
  }
</script>

{#if err}
  <div class="load-err"><span>{err}</span></div>
{:else if (!plugins)}
  <div class="pref-section">
    <div class="skel tall" />
    <div class="skel" />
  </div>
{:else}
  <div class="pref-section">
    <div>
      <h3 class="section-title">Skills</h3>
      <p class="section-desc">Command packs from <b>~/.parzi/plugins</b> — each enabled skill adds slash commands. Toggles apply immediately.</p>
    </div>

    {#if !skills.length}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">No skills installed</span>
          <span class="field-hint">Drop a folder with a <b>parzi-plugin.toml</b> of kind <b>commands</b> into ~/.parzi/plugins.</span>
        </div>
      </div>
    {/if}

    {#each skills as p (p.name)}
      <div class="field-card col">
        <div class="card-top">
          <div class="field-info">
            <span class="field-label">{p.name} <span class="kind">skill · v{p.version}</span></span>
            <span class="field-hint">{p.enabled ? "Slash commands from this pack are live." : "Disabled — commands hidden until re-enabled."}</span>
          </div>
          <div class="row-actions">
            <button class="mini-btn" title={openSkill === p.name ? "Close commands" : `Edit ${p.name} commands`} on:click={() => openCommands(p.name)}>
              <Icon d={I.edit} size={12} />
            </button>
            <Switch on={p.enabled} title={p.enabled ? `Disable ${p.name}` : `Enable ${p.name}`} on:toggle={() => flip(p)} />
          </div>
        </div>
        {#if openSkill === p.name}
          <div class="cmd-zone">
            {#if commands === null && !cmdErr}
              <div class="skel" />
            {:else}
              {#each commands ?? [] as c (c.name)}
                <div class="cmd-row">
                  <div class="field-info">
                    <span class="field-label mono">/{c.name}</span>
                    <span class="field-hint">{c.description}</span>
                  </div>
                  <button class="mini-btn" title={`Remove /${c.name}`} on:click={() => removeCommand(c.name)}>
                    <Icon d={I.trash} size={12} />
                  </button>
                </div>
              {/each}
              {#if !(commands ?? []).length}
                <span class="field-hint">No commands yet — add the first one below.</span>
              {/if}
              <div class="form-grid">
                <div class="form-2col">
                  <input class="txt mono" placeholder="name — e.g. summarize" bind:value={nc.name} />
                  <input class="txt" placeholder="Short description" bind:value={nc.description} />
                </div>
                <textarea class="txt" rows="2" placeholder="Prompt sent when /name runs…" bind:value={nc.prompt} />
                {#if cmdErr}
                  <span class="form-err">{cmdErr}</span>
                {/if}
                <div class="btn-row">
                  <button class="sbtn" on:click={addCommand}><Icon d={I.plus} size={12} /><span>Add command</span></button>
                  <button class="sbtn primary" on:click={saveCommands} disabled={saving}><span>{saving ? "Saving…" : "Save commands"}</span></button>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <div class="pref-section">
    <h3 class="section-title">New skill</h3>
    <div class="field-card">
      <div class="field-info">
        <span class="field-label">Create a command pack</span>
        <span class="field-hint">Scaffolds <b>~/.parzi/plugins/&lt;name&gt;</b> — then add slash commands above.</span>
        {#if createErr}
          <span class="form-err">{createErr}</span>
        {/if}
      </div>
    </div>
    <div class="new-row">
      <input
        class="txt mono"
        placeholder="skill name — e.g. review"
        bind:value={newSkill}
        on:keydown={(e) => e.key === "Enter" && create()}
      />
      <button class="sbtn primary" on:click={create} disabled={creating || !newSkill.trim()}>
        <Icon d={I.plus} size={12} /><span>{creating ? "Creating…" : "Create"}</span>
      </button>
    </div>
  </div>

  {#if packs.length}
    <div class="pref-section">
      <h3 class="section-title">Connector packs</h3>
      <p class="section-desc">Pre-wired MCP server configs. Enable here, then manage servers under <b>Connectors</b>.</p>
      {#each packs as p (p.name)}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label">{p.name} <span class="kind">mcp-pack · v{p.version}</span></span>
          </div>
          <Switch on={p.enabled} title={p.enabled ? `Disable ${p.name}` : `Enable ${p.name}`} on:toggle={() => flip(p)} />
        </div>
      {/each}
    </div>
  {/if}

  {#if themes.length}
    <div class="pref-section">
      <h3 class="section-title">Theme packs</h3>
      <p class="section-desc">Look packs. Apply them under <b>Appearance</b>.</p>
      {#each themes as p (p.name)}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label">{p.name} <span class="kind">theme · v{p.version}</span></span>
          </div>
          <Switch on={p.enabled} title={p.enabled ? `Disable ${p.name}` : `Enable ${p.name}`} on:toggle={() => flip(p)} />
        </div>
      {/each}
    </div>
  {/if}

  {#if other.length}
    <div class="pref-section">
      <h3 class="section-title">Other</h3>
      {#each other as p (p.name)}
        <div class="field-card">
          <div class="field-info">
            <span class="field-label">{p.name} <span class="kind">{p.kind} · v{p.version}</span></span>
          </div>
          <Switch on={p.enabled} title={p.enabled ? `Disable ${p.name}` : `Enable ${p.name}`} on:toggle={() => flip(p)} />
        </div>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .kind {
    font-size: 10px; font-weight: 600; padding: 2px 7px; margin-left: 8px;
    border-radius: var(--radius-pill); white-space: nowrap;
    background: var(--surface-2); color: var(--text-3); border: 1px solid var(--line-3);
  }
  .mono { font-family: var(--parzi-mono), ui-monospace, monospace; }
  .field-card.col { flex-direction: column; align-items: stretch; }
  .card-top { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .row-actions { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  .mini-btn {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 28px; height: 28px; padding: 0 4px;
    background: transparent; border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text-3); cursor: pointer;
  }
  .mini-btn:hover { background: var(--surface-2); color: var(--text); }
  .cmd-zone { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--line-2); padding-top: 10px; }
  .cmd-row {
    display: flex; align-items: center; gap: 10px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-2); padding: 8px 12px;
  }
  .form-grid { display: flex; flex-direction: column; gap: 8px; }
  .form-2col { display: grid; grid-template-columns: 180px 1fr; gap: 8px; }
  .txt {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; padding: 7px 10px; width: 100%;
  }
  textarea.txt { resize: vertical; min-height: 48px; line-height: 1.5; }
  .txt::placeholder { color: var(--text-4); }
  .btn-row { display: flex; gap: 8px; }
  .form-err { font-size: 12px; color: var(--bad); }
  .new-row { display: flex; gap: 8px; }
  .new-row .txt { flex: 1; }
</style>
