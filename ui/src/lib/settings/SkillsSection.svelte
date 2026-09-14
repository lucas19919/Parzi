<script lang="ts">
  import { onMount } from "svelte";
  import { api, type PluginView } from "../api";
  import Switch from "./Switch.svelte";
  import Icon from "../Icon.svelte";
  import "./shared.css";

  export let notify: (msg: string) => void = () => {};

  const I = {
    plus: "M12 5v14M5 12h14",
    trash: "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
  };

  let plugins: PluginView[] | null = null;
  let err = "";
  let busy: Record<string, boolean> = {};

  let paste = "";
  let pasteName = "";
  let pasteErr = "";
  let pasting = false;

  let gitUrl = "";
  let gitErr = "";
  let gitNote = "";
  let installing = false;

  let showComposer = false;
  let nd = { name: "", description: "", body: "" };
  let ndErr = "";
  let ndBusy = false;

  let armDelete: string | null = null;

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

  async function addPasted() {
    pasteErr = "";
    if (pasting || !paste.trim()) return;
    pasting = true;
    try {
      const r = await api.installPastedSkill(pasteName.trim(), paste);
      paste = "";
      pasteName = "";
      await refresh();
      notify(`Added ${r.name} (${r.commands} command${r.commands === 1 ? "" : "s"})`);
    } catch (e) {
      pasteErr = String(e);
    } finally {
      pasting = false;
    }
  }

  async function installFromGit() {
    gitErr = "";
    gitNote = "";
    if (installing || !gitUrl.trim()) return;
    installing = true;
    try {
      const r = await api.installSkillFromGit(gitUrl.trim());
      gitUrl = "";
      await refresh();
      gitNote =
        `Installed: ${r.installed.join(", ") || "—"}` +
        (r.skipped.length ? ` · Skipped: ${r.skipped.join("; ")}` : "");
      notify(`Installed ${r.installed.length} skill${r.installed.length === 1 ? "" : "s"}`);
    } catch (e) {
      gitErr = String(e);
    } finally {
      installing = false;
    }
  }

  async function createMd() {
    ndErr = "";
    const name = nd.name.trim();
    if (!name) {
      ndErr = "Give it a name.";
      return;
    }
    if (!nd.body.trim()) {
      ndErr = "Write the skill first.";
      return;
    }
    if (ndBusy) return;
    ndBusy = true;
    try {
      const md = `---\nname: ${name}\ndescription: ${nd.description.trim()}\n---\n${nd.body.trim()}\n`;
      const r = await api.installPastedSkill(name, md);
      nd = { name: "", description: "", body: "" };
      showComposer = false;
      await refresh();
      notify(`Added ${r.name}`);
    } catch (e) {
      ndErr = String(e);
    } finally {
      ndBusy = false;
    }
  }

  async function removeSkill(name: string) {
    if (armDelete !== name) {
      armDelete = name;
      setTimeout(() => {
        if (armDelete === name) armDelete = null;
      }, 3000);
      return;
    }
    armDelete = null;
    try {
      await api.deleteSkill(name);
      await refresh();
      notify(`Deleted ${name}`);
    } catch (e) {
      notify(`Delete failed: ${e}`);
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
      <p class="section-desc">Slash-command packs. Paste one, write one, or pull a whole library from GitHub.</p>
    </div>

    {#if !skills.length}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">No skills yet</span>
          <span class="field-hint">Paste a SKILL.md below, or install a library.</span>
        </div>
      </div>
    {/if}

    {#each skills as p (p.name)}
      <div class="field-card">
        <div class="field-info">
          <span class="field-label">{p.name} <span class="kind">skill · v{p.version}</span></span>
          <span class="field-hint">
            {p.commands} slash command{p.commands === 1 ? "" : "s"}{p.enabled ? "" : " · off — hidden until re-enabled"}
          </span>
        </div>
        <div class="row-actions">
          <Switch on={p.enabled} title={p.enabled ? `Disable ${p.name}` : `Enable ${p.name}`} on:toggle={() => flip(p)} />
          <button
            class="mini-btn"
            class:armed={armDelete === p.name}
            title={armDelete === p.name ? "Click again to confirm delete" : `Delete ${p.name}`}
            on:click={() => removeSkill(p.name)}
          >
            {#if armDelete === p.name}<span class="arm-txt">Sure?</span>{:else}<Icon d={I.trash} size={12} />{/if}
          </button>
        </div>
      </div>
    {/each}
  </div>

  <div class="pref-section">
    <h3 class="section-title">Add a skill</h3>
    <p class="section-desc">Paste a <b>SKILL.md</b> file or a <b>commands.toml</b> block — name is read from the file when present.</p>
    <div class="field-card col">
      <textarea
        class="txt mono"
        rows="5"
        placeholder={'---\nname: review\ndescription: Review this diff\n---\n# instructions…\n\n(or a commands.toml [[command]] block)'}
        bind:value={paste}
      />
      <div class="paste-row">
        <input class="txt" placeholder="Name (optional — read from the file)" bind:value={pasteName} on:keydown={(e) => e.key === "Enter" && addPasted()} />
        <button class="sbtn primary" on:click={addPasted} disabled={pasting || !paste.trim()}>
          <Icon d={I.plus} size={12} />
          <span>{pasting ? "Adding…" : "Add skill"}</span>
        </button>
      </div>
      {#if pasteErr}
        <span class="form-err">{pasteErr}</span>
      {/if}
    </div>

    <div class="or-row"><span>or</span></div>

    <div class="field-card col">
      <div class="field-info">
        <span class="field-label">Install a library from GitHub</span>
        <span class="field-hint">Clones the repo, finds every skill folder, installs what isn't already there.</span>
      </div>
      <div class="paste-row">
        <input
          class="txt mono"
          placeholder="owner/repo  — or a full https URL"
          bind:value={gitUrl}
          on:keydown={(e) => e.key === "Enter" && installFromGit()}
        />
        <button class="sbtn primary" on:click={installFromGit} disabled={installing || !gitUrl.trim()}>
          <span>{installing ? "Installing…" : "Install"}</span>
        </button>
      </div>
      {#if gitErr}
        <span class="form-err">{gitErr}</span>
      {/if}
      {#if gitNote}
        <span class="field-hint">{gitNote}</span>
      {/if}
    </div>

    <button class="link-btn" on:click={() => (showComposer = !showComposer)}>
      {showComposer ? "Hide editor ▴" : "Or write a new one ▾"}
    </button>
    {#if showComposer}
      <div class="field-card col">
        <div class="form-grid">
          <div class="form-2col">
            <input class="txt mono" placeholder="name — e.g. review" bind:value={nd.name} />
            <input class="txt" placeholder="Short description" bind:value={nd.description} />
          </div>
          <textarea class="txt" rows="6" placeholder="What should this skill do? Write it like a SKILL.md body…" bind:value={nd.body} />
        </div>
        {#if ndErr}
          <span class="form-err">{ndErr}</span>
        {/if}
        <div>
          <button class="sbtn primary" on:click={createMd} disabled={ndBusy || !nd.name.trim() || !nd.body.trim()}>
            <Icon d={I.plus} size={12} />
            <span>{ndBusy ? "Creating…" : "Create skill"}</span>
          </button>
        </div>
      </div>
    {/if}
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
  .row-actions { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  .mini-btn {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 28px; height: 28px; padding: 0 4px;
    background: transparent; border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text-3); cursor: pointer;
  }
  .mini-btn:hover { background: var(--surface-2); color: var(--text); }
  .mini-btn.armed { border-color: var(--bad-line); color: var(--bad); min-width: 52px; }
  .arm-txt { font-size: 11px; font-weight: 600; }
  .form-grid { display: flex; flex-direction: column; gap: 8px; }
  .form-2col { display: grid; grid-template-columns: 180px 1fr; gap: 8px; }
  .paste-row { display: grid; grid-template-columns: 1fr auto; gap: 8px; }
  .txt {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 12.5px; padding: 7px 10px; width: 100%;
  }
  textarea.txt { resize: vertical; min-height: 80px; line-height: 1.5; }
  .txt::placeholder { color: var(--text-4); }
  .form-err { font-size: 12px; color: var(--bad); }
  .or-row { display: flex; align-items: center; gap: 10px; color: var(--text-4); font-size: 11px; }
  .or-row::before, .or-row::after { content: ""; flex: 1; border-top: 1px solid var(--line-2); }
  .link-btn {
    background: transparent; border: none; color: var(--text-3); font: inherit;
    font-size: 12px; cursor: pointer; padding: 2px 0; text-align: left; width: fit-content;
  }
  .link-btn:hover { color: var(--text); }
</style>
