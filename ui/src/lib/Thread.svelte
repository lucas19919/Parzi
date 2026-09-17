<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { renderMarkdown, splitSegments, LiveMarkdown } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import Diagram from "./widgets/Diagram.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import ToolStack from "./ToolStack.svelte";
  import TelemetryRibbon from "./TelemetryRibbon.svelte";
  import type { ChatEvent, InspectorArtifact } from "./api";
  import { api } from "./api";

  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveReasoning = "";
  interface LiveTool {
    id: string;
    name: string;
    label: string;
    running: boolean;
    ok: boolean;
    ms: number;
  }
  /** Status track: live tool calls with backend humanized labels. */
  export let liveTools: LiveTool[] = [];
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

  /** Display label for a persisted tool call. Mirrors the backend
      `humanize_tool_call` (Rust is the source of truth; this covers history
      rows whose events carry raw args). */
  function toolLabel(name: string, args?: unknown): string {
    const a = (args ?? {}) as Record<string, unknown>;
    const s = (k: string) => {
      const v = a[k];
      return typeof v === "string" && v.trim() ? v.trim() : "";
    };
    const oneLine = (v: string, n: number) => {
      const f = v.split("\n")[0].trim();
      return f.length > n ? f.slice(0, n) + "…" : f;
    };
    switch (name) {
      case "fs.read": return `Reading ${s("path") || "?"}`;
      case "fs.write": return `Writing ${s("path") || "?"}`;
      case "fs.list": return `Listing ${s("path") || "."}`;
      case "shell.exec": return `Running \`${oneLine(s("cmd"), 60)}\``;
      case "session.spawn": return `Delegating: ${oneLine(s("title") || "subsession", 60)}`;
      case "session.send_message": return `Messaging ${(s("session_id") || "session").slice(0, 8)}`;
      case "session.read_session": return "Reading session";
      case "session.list_sessions": return "Listing sessions";
      case "plan.read": return "Reading plan";
      case "plan.update": return `Updating plan: ${oneLine(s("title_match") || s("append"), 60)}`;
      case "lane.dispatch": return `Dispatching worker: ${oneLine(s("title") || "worker", 60)}`;
      case "knowledge.read": return "Reading knowledge";
      case "knowledge.record": return "Recording knowledge";
      case "ui.show_markdown": return "Rendering text";
      case "ui.show_widget": return `Rendering ${oneLine(s("title") || s("type") || "widget", 60)}`;
      case "ui.show_diagram": return "Rendering diagram";
      case "ui.show_artifact": return `Saving ${oneLine(s("title") || s("id") || "artifact", 60)}`;
      default: {
        const hint = s("path") || s("file") || s("cmd") || s("query") || s("prompt") || s("title") || s("url") || s("message") || s("note") || s("number") || s("id");
        return hint ? `Calling ${name} ${oneLine(hint, 60)}` : `Calling ${name}`;
      }
    }
  }

  /** `key` is stable for the life of the event it came from (its index in the
      append-only log, or the tool-call id), so a token landing in the live tail
      never re-keys — and so never re-renders — a finished message. */
  type RenderItem =
    | { key: string; kind: "user"; text: string }
    | { key: string; kind: "assistant"; text: string }
    | { key: string; kind: "reasoning"; text: string }
    | { key: string; kind: "checkpoint"; text: string }
    | { key: string; kind: "system"; text: string }
    | { key: string; kind: "route"; text: string }
    | { key: string; kind: "widget"; fence: string; payload: unknown }
    | { key: string; kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown }
    | {
        key: string;
        kind: "tool";
        id: string;
        name: string;
        label: string;
        args?: unknown;
        ok?: boolean;
        ms?: number;
        output?: string;
        running: boolean;
      };

  type ToolItem = Extract<RenderItem, { kind: "tool" }>;
  /** Consecutive tool calls collapse into one stack so a busy turn is a
      single line until expanded. */
  type GroupedItem = RenderItem | { kind: "toolgroup"; key: string; tools: ToolItem[] };

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
    for (let ix = 0; ix < events.length; ix++) {
      const e = events[ix];
      const key = `e${ix}`;
      if (e.kind === "user") {
        out.push({ key, kind: "user", text: e.text });
      } else if (e.kind === "assistant") {
        out.push({ key, kind: "assistant", text: e.text });
      } else if (e.kind === "reasoning") {
        out.push({ key, kind: "reasoning", text: e.text });
      } else if (e.kind === "checkpoint") {
        out.push({ key, kind: "checkpoint", text: e.summary });
      } else if (e.kind === "system") {
        out.push({ key, kind: "system", text: e.text });
      } else if (e.kind === "route_transition") {
        const cd = e.cooldown_secs ? ` (cooldown: ${e.cooldown_secs}s)` : "";
        out.push({ key, kind: "route", text: `${e.from_provider} → ${e.to_provider} (${e.reason}${cd})` });
      } else if (e.kind === "widget") {
        out.push({ key, kind: "widget", fence: e.fence, payload: e.payload });
      } else if (e.kind === "artifact") {
        out.push({ key, kind: "artifact", id: e.id, title: e.title, artifact_kind: e.artifact_kind, version: e.version, payload: e.payload });
      } else if (e.kind === "tool_call") {
        const res = resultMap.get(e.id);
        if (res) {
          out.push({
            key,
            kind: "tool",
            id: e.id,
            name: e.name,
            label: toolLabel(e.name, e.args),
            args: e.args,
            ok: res.ok,
            ms: res.ms,
            output: res.output,
            running: false,
          });
        } else {
          out.push({
            key,
            kind: "tool",
            id: e.id,
            name: e.name,
            label: toolLabel(e.name, e.args),
            args: e.args,
            running: true,
          });
        }
      }
    }
    return out;
  })();

  $: groupedItems = ((): GroupedItem[] => {
    const out: GroupedItem[] = [];
    let buf: ToolItem[] = [];
    const flush = () => {
      if (buf.length) {
        out.push({ kind: "toolgroup", key: "g" + buf.map((t) => t.id).join("+"), tools: buf });
        buf = [];
      }
    };
    for (const it of chronologicalItems) {
      if (it.kind === "tool") buf.push(it);
      else {
        flush();
        out.push(it);
      }
    }
    flush();
    return out;
  })();

  /** The "Now:" line: last running tool's label, else the text-track state. */
  $: liveRunning = liveTools.filter((t) => t.running);
  $: lastRunning = liveRunning.length ? liveRunning[liveRunning.length - 1] : null;
  $: nowText = lastRunning
    ? lastRunning.label + "…"
    : streaming
      ? liveText
        ? "Writing…"
        : liveReasoning
          ? "Thinking…"
          : liveTools.length
            ? "Working…"
            : "Starting…"
      : "";

  /** Live tail (E4). The buffer is split without touching the segment LRU —
      its key changes on every flush, so caching it would evict the finished
      messages it exists to keep — and only the last markdown segment streams:
      the blocks before it are parsed once by `LiveMarkdown` and kept as HTML. */
  const liveMd = new LiveMarkdown();
  // Run over or thread switched: forget the frozen HTML, so the next answer
  // cannot inherit it by sharing a first block with the last one.
  $: if (!liveText) liveMd.reset();
  $: liveSegs = liveText ? splitSegments(liveText, false) : [];
  $: liveTailIdx = (() => {
    for (let i = liveSegs.length - 1; i >= 0; i--) if (liveSegs[i].kind === "md") return i;
    return -1;
  })();
  $: livePart =
    liveTailIdx >= 0
      ? liveMd.render(String(liveSegs[liveTailIdx].body))
      : { head: "", tail: "" };

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
  {#each groupedItems as item, i (item.key)}
    {#if item.kind === "user"}
      {@const sentImgs = attachedImages(item.text)}
      <div class="msg-row user">
        <div class="user-bubble msg">{@html renderMarkdown(stripMarker(item.text))}</div>
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
      <!-- A compaction: the model reads the thread from this summary on. -->
      <details class="think">
        <summary>
          <span class="think-rail" />
          <span>Conversation compacted</span>
          <span class="think-meta">the agent continues from this summary</span>
        </summary>
        <div class="think-body">{item.text}</div>
      </details>
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
    {:else if item.kind === "toolgroup"}
      <ToolStack tools={item.tools} />
    {/if}
  {/each}

  {#if streaming && nowText}
    <div class="now-pill" transition:fade|local={spring}>
      <span class="streaming-dot" /><span>{nowText}</span>
    </div>
  {/if}

  {#if liveTools.length}
    <TelemetryRibbon tools={liveTools} prose={!!liveText} />
  {/if}

  {#if liveReasoning}
    <details class="think" open transition:fade|local={spring}>
      <summary><span class="think-rail" /><span>Thinking</span><span class="think-meta">live</span></summary>
      <div class="think-body">{liveReasoning}</div>
    </details>
  {/if}

  {#if liveText}
    {#each liveSegs as seg, si}
      {#if seg.kind === "md" && si === liveTailIdx}
        <div class="msg">
          {#if livePart.head}<div class="live-head fade">{@html livePart.head}</div>{/if}
          <div class="live-tail">{@html livePart.tail}<span class="stream-caret" /></div>
        </div>
      {:else if seg.kind === "md"}
        <div class="msg">{@html renderMarkdown(seg.body)}</div>
      {:else if seg.kind === "widget"}
        <Widget data={seg.body} />
      {:else if seg.kind === "artifact"}
        <ArtifactCard data={seg.body} on:openInDeck={toDeck} />
      {:else}
        <Diagram data={seg.body} />
      {/if}
    {/each}
  {/if}

  {#if streaming && !liveText && !liveReasoning && !liveTools.length && !nowText}
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
    /* The omnibar docks as a floating overlay (~115px single-line, more
       with attachments or a grown textarea): keep this clearance above the
       tallest common composer so the last lines never slide underneath it. */
    padding: 16px 24px 190px;
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
    min-width: 0;
    overflow-wrap: break-word;
    word-break: break-word;
  }
  /* User prompts render as markdown now (same pipeline as assistant
     messages; `{@html}` nodes are invisible to Svelte's scope analysis, so
     these stay `:global` — plain selectors would warn as unused). */
  .user-bubble :global(h1), .user-bubble :global(h2),
  .user-bubble :global(h3), .user-bubble :global(h4) {
    margin: 0.5em 0 0.3em;
    font-size: 1.02em;
  }
  .user-bubble :global(ul), .user-bubble :global(ol) {
    margin: 0.4em 0;
    padding-left: 20px;
  }
  .user-bubble :global(.codeblock) {
    margin: 0.5em 0;
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
  /* The live tail is two elements (frozen blocks + the block being written)
     so the frozen HTML is never re-set. Keep the seam invisible: the frozen
     part's last paragraph would otherwise lose its bottom margin. */
  :global(.live-head > p:last-child) {
    margin-bottom: 0.55em;
  }
  .live-head.fade { opacity: 0.72; filter: saturate(0.92); }
  .live-tail { color: var(--text); }
  .stream-caret {
    display: inline-block; width: 7px; height: 1em; margin-left: 2px;
    vertical-align: text-bottom; border-radius: 1px;
    background: var(--accent); box-shadow: 0 0 10px var(--accent-glow), 0 0 2px var(--accent);
    animation: beacon 1.1s var(--ease-spring) infinite;
  }
  @keyframes beacon {
    0%, 100% { opacity: 1; filter: brightness(1.15); }
    50% { opacity: 0.35; filter: brightness(0.85); }
  }
  @media (prefers-reduced-motion: reduce) {
    .stream-caret { animation: none; }
    .live-head.fade { opacity: 1; filter: none; }
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
  .now-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    background: var(--accent-soft);
    border: 1px solid var(--accent-mid);
    border-radius: 999px;
    padding: 5px 12px;
    font-size: 12px;
    color: var(--text);
    max-width: 100%;
  }
  .now-pill span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
