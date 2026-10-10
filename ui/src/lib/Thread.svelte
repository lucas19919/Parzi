<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { handleLinkClick } from "./links";
  import { mdHtml, richReady, splitSegments, LiveMarkdown } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import Steps from "./Steps.svelte";
  import Icon from "./Icon.svelte";
  import { api, type ChatEvent, type Question } from "./api";
  import type { Approval, LiveTool, Step } from "./live";
  import { toast, toastError } from "./toast";

  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveReasoning = "";
  export let liveTools: LiveTool[] = [];
  export let streaming = false;
  export let approval: Approval | null = null;
  export let question: Question | null = null;
  export let folder = "";

  const dispatch = createEventDispatcher<{ voted: { key: string }; answered: { key: string }; edited: { text: string }; forked: void }>();

  const ERROR_WORDS: Record<string, string> = {
    auth: "Not signed in",
    rate_limit: "Plan limit reached — wait a bit, then send again",
    overloaded: "The service is overloaded — wait a bit, then send again",
    context_overflow: "The conversation is too long",
    bad_request: "The agent refused the request",
    process: "The agent's program stopped",
    unknown: "The turn failed",
  };
  const ERROR_HINT: Record<string, string> = {
    rate_limit: "Your prompt is kept — nothing was lost.",
  };

  // Vendor errors often trail a raw JSON blob; cut it, it never helped.
  function shortErr(message: string): string {
    const cut = message.search(/\s*\{[\s"]*"errorName"|\s*\{"errorName"|\s*\{"code"/);
    if (cut > 0) return `${message.slice(0, cut).trimEnd()}…`;
    return message.length > 400 ? `${message.slice(0, 400).trimEnd()}…` : message;
  }
  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  const spring = { duration: 160, easing: cubicOut };

  type Item =
    | { key: string; kind: "user" | "assistant" | "checkpoint" | "system"; text: string }
    | { key: string; kind: "error"; class: string; message: string }
    | { key: string; kind: "widget"; fence: string; payload: unknown }
    | { key: string; kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown }
    | { key: string; kind: "steps"; reasoning: string; tools: Step[] };

  let copied = "";
  let saved = "";
  const liveMd = new LiveMarkdown();
  const imageCache = new Map<string, Promise<string | null>>();

  $: items = buildItems(events);
  $: running = liveTools.filter((t) => t.running);
  $: status = !streaming
    ? ""
    : running.length && running[running.length - 1].label.trim()
      ? `${running[running.length - 1].label}…`
      : liveText
        ? ""
        : liveReasoning
          ? "Thinking…"
          : liveTools.length
            ? "Working…"
            : "Starting…";
  $: showLiveSteps = !!status || !!liveReasoning || liveTools.length > 0;
  $: if (!liveText) liveMd.reset();
  $: liveSegs = liveText ? splitSegments(liveText, false) : [];
  $: tailIdx = liveSegs.map((s) => s.kind).lastIndexOf("md");
  $: livePart = tailIdx >= 0 ? liveMd.render(String(liveSegs[tailIdx].body)) : { head: "", tail: "" };

  function arg(args: unknown, ...keys: string[]): string {
    const a = (args ?? {}) as Record<string, unknown>;
    for (const k of keys) {
      const v = a[k];
      if (typeof v === "string" && v.trim()) {
        const line = v.trim().split("\n")[0];
        return line.length > 60 ? `${line.slice(0, 60)}…` : line;
      }
    }
    return "";
  }

  function toolLabel(name: string, args: unknown): string {
    switch (name) {
      case "fs.read":
        return `Reading ${arg(args, "path") || "?"}`;
      case "fs.write":
        return `Writing ${arg(args, "path") || "?"}`;
      case "fs.list":
        return `Listing ${arg(args, "path") || "."}`;
      case "shell.exec":
        return `Running ${arg(args, "cmd") || "a command"}`;
      case "browser.open":
        return `Opening ${arg(args, "url") || "a page"}`;
      case "browser.tabs":
        return "Listing tabs";
      case "browser.read":
        return "Reading the page";
      case "browser.shot":
        return "Looking at the page";
      case "image.generate":
        return `Drawing ${arg(args, "prompt") || "an image"}`;
      case "doc.read":
        return `Reading ${arg(args, "source") || "a document"}`;
      case "models.list":
        return "Checking the bench";
      case "ask.user":
        return `Asking ${arg(args, "question") || "a question"}`;
      case "request.user":
        return `Requesting ${arg(args, "request") || "something"}`;
      case "plan.write":
        return "Writing the plan";
      case "plan.read":
        return "Reading the plan";
      case "session.spawn":
        return `Delegating: ${arg(args, "title") || "a subsession"}`;
      case "session.send":
      case "session.send_message":
        return `Messaging ${arg(args, "title", "session", "session_id") || "a subsession"}`;
      case "session.read":
      case "session.read_session":
        return `Checking on ${arg(args, "title", "session", "session_id") || "a subsession"}`;
      case "session.list":
        return "Listing subsessions";
      case "lane.dispatch":
        return `Dispatching ${arg(args, "lane") || "a lane"}`;
      case "lane.status":
        return "Reading lane state";
      case "memory.review":
        return `Reviewing memory ${arg(args, "path") || "?"}`;
      case "project.create":
        return `Creating ${arg(args, "title") || "a project"}`;
      case "project.archive":
        return `Archiving ${arg(args, "slug") || "a project"}`;
      case "project.delete":
        return `Deleting ${arg(args, "slug") || "a project"}`;
      case "shell.start":
        return `Starting ${arg(args, "cmd") || "a command"}`;
      case "shell.logs":
        return "Reading shell output";
      case "shell.kill":
        return "Stopping a shell";
      case "browser.click":
        return `Clicking ${arg(args, "text", "selector") || "a control"}`;
      case "browser.type":
        return `Typing into ${arg(args, "field", "selector") || "a field"}`;
      case "brain.search":
        return `Searching notes for ${arg(args, "query") || "?"}`;
      case "brain.read":
        return `Reading note ${arg(args, "path") || "?"}`;
      case "brain.list":
        return arg(args, "project") ? `Listing ${arg(args, "project")} notes` : "Listing notes";
      case "brain.write":
        return `Writing note ${arg(args, "path") || "?"}`;
      case "brain.delete":
        return `Deleting note ${arg(args, "path") || "?"}`;
      case "ui.show_markdown":
        return "Rendering text";
      case "ui.show_artifact":
        return `Saving ${arg(args, "title", "id") || "artifact"}`;
      default: {
        const hint = arg(args, "path", "file", "cmd", "query", "url", "title", "id");
        return hint ? `Calling ${name} ${hint}` : `Calling ${name}`;
      }
    }
  }

  function buildItems(events: ChatEvent[]): Item[] {
    const results = new Map<string, { ok: boolean; ms: number; output: string }>();
    for (const e of events) if (e.kind === "tool_result") results.set(e.id, { ok: e.ok, ms: e.ms, output: e.output });
    const out: Item[] = [];
    let group: Extract<Item, { kind: "steps" }> | null = null;
    const steps = (key: string) => {
      if (!group) {
        group = { key: `s${key}`, kind: "steps", reasoning: "", tools: [] };
        out.push(group);
      }
      return group;
    };
    events.forEach((e, i) => {
      const key = `e${i}`;
      if (e.kind === "tool_call") {
        const r = results.get(e.id);
        steps(key).tools.push({ id: e.id, label: toolLabel(e.name, e.args), running: !r, ...r });
        return;
      }
      if (e.kind === "reasoning") {
        const g = steps(key);
        g.reasoning = g.reasoning ? `${g.reasoning}

${e.text}` : e.text;
        return;
      }
      if (e.kind === "tool_result") return;
      group = null;
      if (e.kind === "checkpoint") out.push({ key, kind: "checkpoint", text: e.summary });
      else if (e.kind === "error") out.push({ key, kind: "error", class: e.class, message: e.message });
      else if (e.kind === "widget") out.push({ key, kind: "widget", fence: e.fence, payload: e.payload });
      else if (e.kind === "artifact") out.push({ key, ...e });
      else if (e.kind === "assistant" && echoLine(e.text)) {
        pushSys(key, out, echoLine(e.text));
      } else if (e.kind === "system") {
        pushSys(key, out, e.text);
      } else out.push({ key, kind: e.kind, text: e.text });
    });
    return out;
  }

  // Consecutive routine lines join into one whisper instead of a stack.
  function pushSys(key: string, out: Item[], text: string) {
    const prev = out[out.length - 1];
    if (prev?.kind === "system" && sysTone(prev.text) === "line" && sysTone(text) === "line") {
      prev.text = `${prev.text} · ${text.trim()}`;
      return;
    }
    out.push({ key, kind: "system", text });
  }

  function attachedImages(text: string): string[] {
    const m = /\[attached: ([^\]]+)\]/.exec(text);
    return m ? m[1].split(",").map((s) => s.trim()).filter((s) => IMAGE.test(s)) : [];
  }

  // Routine narrations render as a whisper, not a mono block.
  function sysTone(text: string): "line" | "block" {
    return /^(asked the user|the user answered|plan written|plan saved|brain\.write|session\.spawn|session\.send_message|lane\.dispatch):/i.test(
      text.trim(),
    )
      ? "line"
      : "block";
  }

  function stripMarker(text: string): string {
    return text.replace(/\n?\[attached: [^\]]+\]/, "").trimEnd();
  }

  // Agent narration that only echoes tool I/O (a pseudo-call or a result
  // JSON pasted into chat) carries no information — whisper one line.
  function echoLine(text: string): string {
    const t = text.trim();
    if (/^[\w.]+\(\s*['"`{]/.test(t) || (t.length < 500 && /"(session_id|response)"\s*:/.test(t))) {
      const line = t.split("\n")[0].trim();
      return line.length > 140 ? `${line.slice(0, 140)}…` : line;
    }
    return "";
  }

  function imageUrl(name: string): Promise<string | null> {
    const key = `${folder}\n${name}`;
    let p = imageCache.get(key);
    if (!p) {
      p = api.readImageDataUrl(name, folder).catch(() => null);
      imageCache.set(key, p);
    }
    return p;
  }

  function copy(text: string, key: string) {
    navigator.clipboard.writeText(text).then(() => {
      copied = key;
      setTimeout(() => (copied = copied === key ? "" : copied), 1200);
    }, (e) => toast(`Couldn't copy: ${e}`, true));
  }

  async function save(text: string, key: string) {
    if (saved) return;
    try {
      const note = await api.saveAnswer(text);
      saved = key;
      setTimeout(() => (saved = saved === key ? "" : saved), 1600);
      toast(`Saved to brain as ${note.title || note.path}`);
    } catch (e) {
      toastError(e);
    }
  }

  async function vote(allow: boolean) {
    if (!approval) return;
    const { key, session } = approval;
    try {
      await api.approveTool(key, session, allow);
    } catch (e) {
      toastError(e);
    } finally {
      dispatch("voted", { key });
    }
  }

  let answerText = "";

  async function respond(answer: string) {
    if (!question || !answer.trim()) return;
    const { key, session } = question;
    answerText = "";
    try {
      await api.answerQuestion(key, session, answer.trim());
    } catch (e) {
      toastError(e);
    } finally {
      dispatch("answered", { key });
    }
  }

  async function onClick(e: MouseEvent) {
    if (await handleLinkClick(e)) return;
    const el = e.target as HTMLElement;
    const copyBtn = el.closest<HTMLElement>("[data-copy]");
    if (copyBtn) {
      const code = copyBtn.closest(".codeblock")?.querySelector("code")?.textContent ?? "";
      navigator.clipboard.writeText(code).then(() => {
        copyBtn.textContent = "Copied";
        setTimeout(() => (copyBtn.textContent = "Copy"), 1200);
      }, (e) => toast(`Couldn't copy: ${e}`, true));
      return;
    }
    const expand = el.closest<HTMLElement>("[data-expand]");
    if (expand) {
      const block = expand.closest(".codeblock");
      block?.classList.toggle("clamped");
      expand.textContent = block?.classList.contains("clamped") ? (expand.dataset.label ?? "Show all") : "Show less";
    }
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
<div class="thread-col" on:click={onClick}>
  {#each items as item (item.key)}
    {#if item.kind === "user"}
      {@const images = attachedImages(item.text)}
      <div class="row user">
        <div class="user-line">
          <button class="copy" title="Copy" on:click={() => copy(stripMarker(item.text), item.key)}>
            <Icon name={copied === item.key ? "check" : "copy"} size={13} />
          </button>
          <button class="copy" title="Edit in composer" on:click={() => dispatch("edited", { text: stripMarker(item.text) })}>
            <Icon name="pencil" size={13} />
          </button>
          <div class="bubble msg">{@html mdHtml(stripMarker(item.text), $richReady)}</div>
        </div>
        {#if images.length}
          <div class="sent">
            {#each images as name}
              {#await imageUrl(name) then url}
                {#if url}<img src={url} alt={name} title={name} />{/if}
              {/await}
            {/each}
          </div>
        {/if}
      </div>
    {:else if item.kind === "assistant"}
      <div class="row">
        <div class="body">
          {#each splitSegments(item.text) as seg}
            {#if seg.kind === "md"}
              <div class="msg">{@html mdHtml(seg.body, $richReady)}</div>
            {:else if seg.kind === "widget"}
              <Widget data={seg.body} />
            {:else if seg.kind === "artifact"}
              <ArtifactCard data={seg.body} />
            {/if}
          {/each}
        </div>
        <div class="actions">
          <button class="copy" title="Copy" on:click={() => copy(item.text, item.key)}>
            <Icon name={copied === item.key ? "check" : "copy"} size={13} />
          </button>
          <button class="copy" title="Save to brain" on:click={() => save(item.text, item.key)}>
            <Icon name={saved === item.key ? "check" : "brain"} size={13} />
          </button>
          <button class="copy" title="Fork thread to try again" on:click={() => dispatch("forked")}>
            <Icon name="fork" size={13} />
          </button>
        </div>
      </div>
    {:else if item.kind === "steps"}
      <Steps reasoning={item.reasoning} tools={item.tools} />
    {:else if item.kind === "checkpoint"}
      <Steps title="Conversation compacted" reasoning={item.text} />
    {:else if item.kind === "system"}
      {#if sysTone(item.text) === "line"}
        <div class="sysline">{item.text.slice(0, 300)}</div>
      {:else}
        <div class="system">{item.text.slice(0, 300)}</div>
      {/if}
    {:else if item.kind === "error"}
      <div class="turn-error" role="alert">
        <span class="err-class">{ERROR_WORDS[item.class] ?? ERROR_WORDS.unknown}</span>
        <span class="err-msg">{shortErr(item.message)}</span>
        {#if ERROR_HINT[item.class]}<span class="err-hint">{ERROR_HINT[item.class]}</span>{/if}
      </div>
    {:else if item.kind === "widget"}
      {#if item.fence === "parzi-widget"}
        <Widget data={item.payload} />
      {:else}
        <div class="msg">{@html mdHtml("```" + item.fence + "\n" + JSON.stringify(item.payload, null, 2) + "\n```", $richReady)}</div>
      {/if}
    {:else if item.kind === "artifact"}
      <ArtifactCard
        data={item.payload}
        fallbackId={item.id}
        fallbackTitle={item.title}
        fallbackKind={item.artifact_kind}
      />
    {/if}
  {/each}

  {#if showLiveSteps}
    <div transition:fade|local={spring}>
      <Steps reasoning={liveReasoning} tools={liveTools} {status} />
    </div>
  {/if}

  {#each liveSegs as seg, i}
    {#if seg.kind === "md" && i === tailIdx}
      <div class="msg">
        {#if livePart.head}<div class="live-head">{@html livePart.head}</div>{/if}
        <div class="live-tail">{@html livePart.tail}</div>
      </div>
    {:else if seg.kind === "md"}
      <div class="msg">{@html mdHtml(seg.body, $richReady)}</div>
    {:else if seg.kind === "widget"}
      <Widget data={seg.body} />
    {:else if seg.kind === "artifact"}
      <ArtifactCard data={seg.body} />
    {/if}
  {/each}

  {#if approval}
    <div class="approval" transition:fade={spring}>
      <div>Allow <b>{approval.call.name}</b>?</div>
      <pre>{JSON.stringify(approval.call.args, null, 2)?.slice(0, 2000)}</pre>
      <div class="approval-actions">
        <button class="btn primary" on:click={() => vote(true)}>Approve</button>
        <button class="btn" on:click={() => vote(false)}>Deny</button>
      </div>
    </div>
  {/if}

  {#if question}
    <div class="approval ask" transition:fade={spring}>
      <div class="ask-q">{question.question}</div>
      {#if question.options.length}
        <div class="ask-options">
          {#each question.options as opt}
            <button class="btn" on:click={() => respond(opt)}>{opt}</button>
          {/each}
        </div>
      {/if}
      <div class="ask-free">
        <input
          placeholder="Or type an answer… (Enter sends)"
          bind:value={answerText}
          on:keydown={(e) => {
            if (e.key === "Enter") respond(answerText);
          }}
        />
        <button class="btn primary" disabled={!answerText.trim()} on:click={() => respond(answerText)}>Send</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .thread-col {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
    padding: 16px 24px 8px;
  }
  .row {
    display: flex;
    flex-direction: column;
  }
  .row.user {
    align-items: flex-end;
  }
  .user-line {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    max-width: 85%;
  }
  .bubble {
    min-width: 0;
    max-width: 100%;
    padding: 8px 16px;
    background: var(--line);
    border: 1px solid var(--line);
    border-radius: 14px;
    color: var(--text);
    font-size: 13.5px;
    line-height: 1.5;
    overflow-wrap: break-word;
  }
  .bubble:has(p + p, pre, ul, ol, blockquote) {
    border-radius: 12px;
  }
  .bubble :global(h1),
  .bubble :global(h2),
  .bubble :global(h3) {
    margin: 0.5em 0 0.3em;
    font-size: 1.02em;
  }
  .bubble :global(ul),
  .bubble :global(ol) {
    margin: 0.4em 0;
    padding-left: 20px;
  }
  .sent {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 6px;
  }
  .sent img {
    max-width: 220px;
    max-height: 160px;
    object-fit: cover;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: color-mix(in srgb, var(--text) 92%, transparent);
  }
  .actions {
    height: 24px;
    margin-left: -4px;
    display: flex;
    align-items: center;
  }
  .copy {
    width: 24px;
    height: 24px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .row:hover .copy,
  .copy:focus-visible {
    opacity: 1;
  }
  .copy:hover {
    background: var(--line);
    color: var(--text);
  }
  .live-head {
    opacity: 0.72;
  }
  .live-head :global(p:last-child) {
    margin-bottom: 0.55em;
  }
  .live-tail {
    color: var(--text);
  }
  .system {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--muted);
  }
  .sysline {
    font-size: 12px;
    color: var(--faint);
    padding-left: 2px;
  }
  .turn-error {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--bad);
    border-radius: var(--radius);
    font-size: 12px;
  }
  .err-class {
    font-weight: 600;
    color: var(--bad);
  }
  .err-msg {
    color: var(--muted);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .err-hint {
    color: var(--faint);
    font-size: 11.5px;
  }
  .approval {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    background: transparent;
    border: 1px solid var(--warn);
    border-radius: var(--radius-lg);
    font-size: 13px;
    color: var(--text);
  }
  .approval pre {
    max-height: 160px;
    margin: 0;
    padding: 8px 12px;
    overflow: auto;
    background: var(--bg);
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: 11px;
  }
  .approval-actions {
    display: flex;
    gap: 8px;
  }
  .ask-q {
    font-weight: 600;
  }
  .ask-options {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .ask-free {
    display: flex;
    gap: 8px;
  }
  .ask-free input {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .ask-free input:focus {
    border-color: var(--accent);
  }
  @media (prefers-reduced-motion: reduce) {
    .live-head {
      opacity: 1;
    }
  }
</style>
