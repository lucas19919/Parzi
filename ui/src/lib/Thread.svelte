<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { renderMarkdown, splitSegments } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import Diagram from "./widgets/Diagram.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import type { ChatEvent, InspectorArtifact } from "./api";
  import { api } from "./api";

  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveReasoning = "";
  export let streaming = false;
  export let approval: { key: string; call: { id: string; name: string; args: unknown; lane: string } } | null = null;
  /** Set when this thread is a subsession: title of the parent session. */
  export let parentTitle: string | null = null;
  /** Workspace root: resolves `[attached: …]` image markers to previews. */
  export let projectRoot = "";
  /** Child subsessions of this thread (team status banner). */
  export let subsessions: { id: string; title: string; status: string }[] = [];

  const dispatch = createEventDispatcher<{
    goParent: void;
    openSubsession: { id: string };
    /** Artifact card asked to be read in the right inspector deck. */
    openArtifact: { artifact: InspectorArtifact };
    /** Team banner asked for the swarm view. */
    inspectSwarm: void;
  }>();

  function toDeck(e: CustomEvent<{ artifact: InspectorArtifact }>) {
    dispatch("openArtifact", { artifact: e.detail.artifact });
  }

  function subState(s: string): string {
    if (s === "active") return "● working";
    if (s === "queued") return "… queued";
    if (s === "done") return "✓ finished";
    if (s === "killed") return "✕ stopped";
    return "○ idle";
  }

  const spring = { duration: 160, easing: cubicOut };
  let copied = -1;
  let openTools: Set<string> = new Set();

  function copy(text: string, i: number) {
    try {
      const p = navigator.clipboard.writeText(text) as unknown as Promise<void> | undefined;
      if (p && typeof p.catch === "function") p.catch(() => {});
    } catch {}
    copied = i;
    setTimeout(() => (copied = -1), 1200);
  }

  async function vote(allow: boolean) {
    if (!approval) return;
    await api.approveTool(approval.key, allow);
    approval = null;
  }

  function onThreadClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const copyBtn = el.closest("[data-copy]") as HTMLElement | null;
    if (copyBtn) {
      const code = copyBtn.closest(".codeblock")?.querySelector("code")?.innerText ?? "";
      navigator.clipboard.writeText(code);
      copyBtn.textContent = "copied";
      setTimeout(() => (copyBtn.textContent = "copy"), 1200);
      return;
    }
    const exp = el.closest("[data-expand]") as HTMLElement | null;
    if (exp) {
      const block = exp.closest(".codeblock");
      block?.classList.toggle("clamped");
      exp.textContent = block?.classList.contains("clamped")
        ? exp.dataset.label ?? "expand"
        : "collapse";
    }
  }

  function toolIcon(name: string): string {
    if (name.startsWith("fs.")) return "file";
    if (name.startsWith("shell")) return "term";
    if (name.startsWith("ui.")) return "eye";
    return "globe";
  }

  function fmtMs(ms: number): string {
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`;
  }

  type RenderItem =
    | { kind: "user"; text: string }
    | { kind: "assistant"; text: string }
    | { kind: "reasoning"; text: string }
    | { kind: "checkpoint"; text: string }
    | { kind: "system"; text: string }
    | { kind: "route"; text: string }
    | { kind: "widget"; fence: string; payload: unknown }
    | { kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown }
    | {
        kind: "tool";
        id: string;
        name: string;
        args?: unknown;
        ok?: boolean;
        ms?: number;
        output?: string;
        running: boolean;
      };

  $: chronologicalItems = (() => {
    const out: RenderItem[] = [];
    const resultMap = new Map<string, { ok: boolean; ms: number; output: string }>();

    // Index results
    for (const e of events) {
      if (e.kind === "tool_result") {
        resultMap.set(e.id, { ok: e.ok, ms: e.ms ?? 0, output: e.output });
      }
    }

    // Build ordered list
    for (const e of events) {
      if (e.kind === "user") {
        out.push({ kind: "user", text: e.text });
      } else if (e.kind === "assistant") {
        out.push({ kind: "assistant", text: e.text });
      } else if (e.kind === "reasoning") {
        out.push({ kind: "reasoning", text: e.text });
      } else if (e.kind === "checkpoint") {
        out.push({ kind: "checkpoint", text: e.summary });
      } else if (e.kind === "system") {
        out.push({ kind: "system", text: e.text });
      } else if (e.kind === "route_transition") {
        const cd = e.cooldown_secs ? ` (cooldown: ${e.cooldown_secs}s)` : "";
        out.push({ kind: "route", text: `${e.from_provider} → ${e.to_provider} (${e.reason}${cd})` });
      } else if (e.kind === "widget") {
        out.push({ kind: "widget", fence: e.fence, payload: e.payload });
      } else if (e.kind === "artifact") {
        out.push({ kind: "artifact", id: e.id, title: e.title, artifact_kind: e.artifact_kind, version: e.version, payload: e.payload });
      } else if (e.kind === "tool_call") {
        const res = resultMap.get(e.id);
        if (res) {
          out.push({
            kind: "tool",
            id: e.id,
            name: e.name,
            args: e.args,
            ok: res.ok,
            ms: res.ms,
            output: res.output,
            running: false,
          });
        } else {
          out.push({
            kind: "tool",
            id: e.id,
            name: e.name,
            args: e.args,
            running: true,
          });
        }
      }
    }
    return out;
  })();

  function toggleTool(id: string) {
    openTools = new Set(openTools);
    if (openTools.has(id)) openTools.delete(id);
    else openTools.add(id);
  }

  const SENT_IMG_RE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  /** Filenames from the backend's `[attached: a, b]` transcript marker. */
  function attachedImages(text: string): string[] {
    const m = /\[attached: ([^\]]+)\]/.exec(text);
    if (!m) return [];
    return m[1].split(",").map((s) => s.trim()).filter((s) => SENT_IMG_RE.test(s));
  }
  function stripMarker(text: string): string {
    return text.replace(/\n?\[attached: [^\]]+\]/, "").trimEnd();
  }
  const sentImgCache = new Map<string, Promise<string | null>>();
  function sentImgUrl(name: string): Promise<string | null> {
    const key = `${projectRoot}\n${name}`;
    let p = sentImgCache.get(key);
    if (!p) {
      p = api.readImageDataUrl(name, projectRoot).catch(() => null);
      sentImgCache.set(key, p);
    }
    return p;
  }
</script>

<div class="thread-col" on:click={onThreadClick}>
  {#if parentTitle}
    <button class="crumb" on:click={() => dispatch("goParent")} title="Back to parent thread">
      <span class="crumb-branch">↳</span><span>Subsession of</span><b>{parentTitle}</b>
    </button>
  {/if}
  {#if subsessions.length}
    <div class="team-banner">
      <div class="team-head">
        <span>Team subsessions</span>
        <button class="team-inspect" title="Open the swarm view in the inspector deck" on:click|stopPropagation={() => dispatch("inspectSwarm")}>Inspect swarm ↗</button>
      </div>
      {#each subsessions as s (s.id)}
        <button class="team-row" on:click={() => dispatch("openSubsession", { id: s.id })} title="Open subsession">
          <span class="team-dot" class:live={s.status === "active" || s.status === "queued"} />
          <span class="team-title">{s.title || "untitled"}</span>
          <span class="team-state">{subState(s.status)}</span>
        </button>
      {/each}
    </div>
  {/if}
  {#each chronologicalItems as item, i (i)}
    {#if item.kind === "user"}
      {@const sentImgs = attachedImages(item.text)}
      <div class="msg-row user">
        <div class="user-bubble">{stripMarker(item.text)}</div>
        {#if sentImgs.length}
          <div class="sent-imgs">
            {#each sentImgs as im}
              {#await sentImgUrl(im) then url}
                {#if url}<img src={url} alt={im} title={im} class="sent-img" />{/if}
              {/await}
            {/each}
          </div>
        {/if}
        <button class="copy-btn" on:click={() => copy(stripMarker(item.text), i)}>
          {copied === i ? "copied" : "copy"}
        </button>
      </div>
    {:else if item.kind === "assistant"}
      <div class="msg-row">
        <div class="msg-body">
          {#each splitSegments(item.text) as seg}
            {#if seg.kind === "md"}
              <div class="msg">{@html renderMarkdown(seg.body)}</div>
            {:else if seg.kind === "widget"}
              <Widget data={seg.body} />
            {:else if seg.kind === "artifact"}
              <ArtifactCard data={seg.body} on:openInDeck={toDeck} />
            {:else}
              <Diagram data={seg.body} />
            {/if}
          {/each}
        </div>
        <button class="copy-btn" on:click={() => copy(item.text, i)}>
          {copied === i ? "copied" : "copy"}
        </button>
      </div>
    {:else if item.kind === "reasoning"}
      <details class="think" transition:fade|local={spring}>
        <summary>
          <span class="think-rail" />
          <span>Thinking</span>
          <span class="think-meta">{item.text.length} chars</span>
        </summary>
        <div class="think-body">{item.text}</div>
      </details>
    {:else if item.kind === "checkpoint"}
      <div class="tool-line">◆ checkpoint: {item.text.slice(0, 200)}</div>
    {:else if item.kind === "system"}
      <div class="tool-line">ⓘ {item.text.slice(0, 300)}</div>
    {:else if item.kind === "route"}
      <div class="tool-line route">⇄ {item.text.slice(0, 300)}</div>
    {:else if item.kind === "widget"}
      {#if item.fence === "parzi-diagram"}
        <Diagram data={item.payload} />
      {:else}
        <Widget data={item.payload} />
      {/if}
    {:else if item.kind === "artifact"}
      <ArtifactCard data={item.payload} fallbackId={item.id} fallbackTitle={item.title} fallbackKind={item.artifact_kind} fallbackVersion={item.version} on:openInDeck={toDeck} />
    {:else if item.kind === "tool"}
      <div
        class="tool-card"
        class:running={item.running}
        class:bad={!item.running && !item.ok}
        transition:fade|local={spring}
      >
        <span class="tool-ico {toolIcon(item.name)}">
          {#if toolIcon(item.name) === "file"}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></svg>
          {:else if toolIcon(item.name) === "term"}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 17l6-5-6-5M12 19h8" /></svg>
          {:else if toolIcon(item.name) === "eye"}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
          {:else}
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3c3 3.5 3 14 0 18M12 3c-3 3.5-3 14 0 18" /></svg>
          {/if}
        </span>
        <span class="tool-name">{item.name}</span>
        {#if item.running}
          <span class="tool-pill run"><span class="pulse-dot" />running</span>
        {:else}
          <button class="tool-pill {item.ok ? 'ok' : 'bad'}" on:click={() => toggleTool(item.id)}>
            {item.ok ? "✓" : "✗"} {fmtMs(item.ms ?? 0)}
          </button>
          <button class="tool-caret" on:click={() => toggleTool(item.id)}>
            {openTools.has(item.id) ? "▾" : "▸"}
          </button>
        {/if}
      </div>
      {#if !item.running && item.output && openTools.has(item.id)}
        <div class="tool-output-box" transition:fade|local={spring}>
          {@html renderMarkdown("```\n" + item.output.slice(0, 6000) + "\n```")}
        </div>
      {/if}
    {/if}
  {/each}

  {#if liveReasoning}
    <details class="think" open transition:fade|local={spring}>
      <summary><span class="think-rail" /><span>Thinking</span><span class="think-meta">live</span></summary>
      <div class="think-body">{liveReasoning}</div>
    </details>
  {/if}

  {#if liveText}
    {#each splitSegments(liveText) as seg}
      {#if seg.kind === "md"}
        <div class="msg">{@html renderMarkdown(seg.body)}<span class="stream-caret" /></div>
      {:else if seg.kind === "widget"}
        <Widget data={seg.body} />
      {:else if seg.kind === "artifact"}
        <ArtifactCard data={seg.body} on:openInDeck={toDeck} />
      {:else}
        <Diagram data={seg.body} />
      {/if}
    {/each}
  {/if}

  {#if streaming && !liveText && !liveReasoning}
    <div class="streaming-row"><span class="streaming-dot" /><span>writing…</span></div>
  {/if}

  {#if approval}
    <div class="approval-card" transition:fade={spring}>
      <div>Allow <b>{approval.call.name}</b> in lane {approval.call.lane}?</div>
      <pre>{JSON.stringify(approval.call.args, null, 2)?.slice(0, 2000)}</pre>
      <button class="btn primary" on:click={() => vote(true)}>Approve</button>
      <button class="btn" on:click={() => vote(false)}>Deny</button>
    </div>
  {/if}
</div>

<style>
  .thread-col {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px 24px 120px;
    max-width: 820px;
    margin: 0 auto;
    width: 100%;
    box-sizing: border-box;
  }
  .msg-row {
    display: flex;
    flex-direction: column;
    position: relative;
  }
  .msg-row.user {
    align-items: flex-end;
  }
  .user-bubble {
    background: var(--accent-soft);
    border: 1px solid var(--accent-mid);
    color: var(--text);
    padding: 10px 16px;
    border-radius: 14px 14px 2px 14px;
    font-size: 13.5px;
    line-height: 1.5;
    max-width: 80%;
    word-break: break-word;
  }
  .sent-imgs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
    margin-top: 6px;
  }
  .sent-img {
    max-width: 220px;
    max-height: 160px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    object-fit: cover;
  }
  .msg-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .copy-btn {
    position: absolute;
    top: 0;
    right: 0;
    opacity: 0;
    background: var(--surface-3);
    border: none;
    border-radius: 4px;
    color: var(--text-3);
    font-size: 11px;
    padding: 2px 6px;
    cursor: pointer;
    transition: opacity 0.12s ease;
  }
  .msg-row:hover .copy-btn {
    opacity: 1;
  }
  .tool-line.route {
    color: var(--accent);
  }
  .think {
    background: var(--input);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
  }
  .think summary {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    color: var(--text-3);
    font-weight: 500;
  }
  .think-rail {
    width: 3px;
    height: 12px;
    background: var(--accent);
    border-radius: 2px;
  }
  .think-meta {
    font-size: 10px;
    opacity: 0.5;
    font-family: var(--parzi-mono), ui-monospace, monospace;
  }
  .think-body {
    margin-top: 8px;
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 11.5px;
    line-height: 1.6;
    color: var(--text-2);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .tool-card {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 12px;
    max-width: fit-content;
  }
  .tool-card.bad {
    border-color: var(--bad-line);
    background: var(--bad-soft);
  }
  .tool-ico {
    display: flex;
    align-items: center;
    color: var(--text-3);
  }
  .tool-name {
    font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 11px;
    color: var(--text);
  }
  .tool-pill {
    background: var(--surface-2);
    border: none;
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 10px;
    font-family: var(--parzi-mono), ui-monospace, monospace;
    color: var(--text-3);
    cursor: pointer;
  }
  .tool-pill.ok {
    color: var(--ok);
    background: var(--ok-soft);
  }
  .tool-pill.bad {
    color: var(--bad);
    background: var(--bad-soft);
  }
  .tool-caret {
    background: transparent;
    border: none;
    color: var(--text-3);
    cursor: pointer;
    font-size: 12px;
    padding: 0 2px;
  }
  .tool-output-box {
    margin-top: 4px;
    font-size: 11px;
  }
  .streaming-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-3);
  }
  .streaming-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1s infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.1); }
  }
  .approval-card {
    background: var(--warn-soft);
    border: 1px solid var(--warn-line);
    border-radius: 10px;
    padding: 14px 16px;
    font-size: 13px;
    color: var(--text);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .crumb {
    align-self: flex-start; display: inline-flex; align-items: center; gap: 7px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: 8px; color: var(--text-2); font: inherit; font-size: 12px;
    padding: 5px 11px; cursor: pointer;
  }
  .crumb:hover { color: var(--text); background: var(--surface-3); }
  .crumb-branch { color: var(--text-3); }
  .crumb b { font-weight: 600; color: var(--text); }
  .team-banner {
    display: flex; flex-direction: column; gap: 2px; padding: 8px;
    background: var(--accent-soft); border: 1px solid var(--accent-mid);
    border-radius: 10px;
  }
  .team-head { font-size: 11px; color: var(--text-3); padding: 0 4px 4px; display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .team-inspect {
    background: transparent; border: none; border-radius: 5px; color: var(--accent);
    font: inherit; font-size: 11px; padding: 1px 6px; cursor: pointer; white-space: nowrap;
  }
  .team-inspect:hover { background: var(--accent-soft); color: var(--text); }
  .team-row {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: transparent; border: none; border-radius: 7px; color: var(--text-2);
    font: inherit; font-size: 12.5px; padding: 6px 8px; cursor: pointer; text-align: left;
  }
  .team-row:hover { background: var(--surface-2); color: var(--text); }
  .team-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--text-4); }
  .team-dot.live { background: var(--ok); box-shadow: 0 0 8px var(--ok-line); }
  .team-title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .team-state { font-size: 11px; color: var(--text-3); font-family: var(--parzi-mono), ui-monospace, monospace; flex: none; }
  .approval-card pre {
    background: var(--input);
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 11px;
    max-height: 160px;
    overflow: auto;
  }
</style>
