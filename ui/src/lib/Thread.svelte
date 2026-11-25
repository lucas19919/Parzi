<script lang="ts">
  import { SLIDE } from "./motion";
  import { createEventDispatcher, onDestroy } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { handleLinkClick } from "./links";
  import { codeBlockClick, fenced, htmlBlocks, liveHtml, mdHtml, richReady, splitSegments } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import Viz from "./viz/Viz.svelte";
  import Steps from "./Steps.svelte";
  import Icon from "./Icon.svelte";
  import { api, type AgentRequest, type ChatEvent, type Proposal, type ProviderStatus, type Question } from "./api";
  import type { Approval, LiveTool, Step } from "./live";
  import { splitQuotes } from "./oldQuotes";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";
  import { formatTime } from "./time";
  import { viewImage } from "./imageViewer";
  import ReplyButtons from "./ReplyButtons.svelte";
  import DraftCard from "./DraftCard.svelte";
  import QuestionCard from "./QuestionCard.svelte";
  import ToolPictures, { picturesIn } from "./ToolPictures.svelte";
  import TestCard from "./TestCard.svelte";
  import { testRunOf, type TestRun } from "./testRuns";
  import { board, nextStep, setupWaits } from "./providerStore";
  import { foldTurns, turnLabel, type Item } from "./turns";

  function when(at: number | undefined): string {
    if (!at) return "";
    return formatTime(at * 1000);
  }

  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveReasoning = "";
  export let liveTools: LiveTool[] = [];
  export let streaming = false;
  export let session = "";
  export let liveArtifacts: { id: string; title: string; artifact_kind: string; version: number; payload: unknown }[] = [];
  /** Set when this session is a draft: it has not run yet. */
  export let draft: { title: string; brief: string } | null = null;
  export let draftStarting = false;
  export let approval: Approval | null = null;
  export let question: Question | null = null;
  /** A plan the agent proposed, waiting on the session for an Accept. */
  export let proposal: Proposal | null = null;
  export let request: AgentRequest | null = null;
  export let askerTitle = "";
  export let agentName = "";
  let rawOpen = false;
  $: if (!approval) rawOpen = false;
  export let folder = "";
  export let provider = "";

  const dispatch = createEventDispatcher<{
    voted: { key: string };
    answered: { key: string; session?: string; text?: string };
    proposal: { session: string };
    edited: { text: string };
    forked: void;
    startDraft: void;
    removeDraft: void;
    retry: void;
    compact: void;
    reply: { text: string };
    openSession: { id: string };
    rewind: { nth: number; text: string };
    pickModel: void;
  }>();

  // The board is a parameter so the template re-runs this when limits arrive.
  function resetLine(p: string, all: ProviderStatus[]): string {
    const windows = all.find((b) => b.provider === p)?.usage ?? [];
    const now = Date.now() / 1000;
    const spent = windows.filter((w) => w.used_percent >= 100 && (w.resets_at ?? 0) > now);
    const pick = (spent.length ? spent : [...windows].sort((a, b) => b.used_percent - a.used_percent).slice(0, 1)).filter((w) => w.resets_at && w.resets_at > now);
    const at = Math.min(...pick.map((w) => w.resets_at ?? Infinity));
    if (!Number.isFinite(at)) return "";
    const mins = Math.round((at - now) / 60);
    const when = new Date(at * 1000);
    const clock = when.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    const day = when.toDateString() === new Date().toDateString() ? "" : ` ${when.toLocaleDateString([], { weekday: "short" })}`;
    return mins < 60 ? `Resets in ${Math.max(1, mins)} min.` : `Resets at ${clock}${day}.`;
  }

  const ERROR_WORDS: Record<string, string> = {
    auth: "Not signed in",
    rate_limit: "Usage limit reached",
    overloaded: "The service is overloaded",
    context_overflow: "The conversation is too long",
    bad_request: "The request didn't go through",
    process: "The agent's program stopped",
    session_lost: "The agent lost this conversation",
    unknown: "The turn failed",
  };
  const RETRYABLE = new Set(["rate_limit", "overloaded", "process", "session_lost", "unknown"]);
  function shortErr(message: string): string {
    const cut = message.search(/\s*\{[\s"]*"errorName"|\s*\{"errorName"|\s*\{"code"/);
    if (cut > 0) return `${message.slice(0, cut).trimEnd()}…`;
    return message.length > 400 ? `${message.slice(0, 400).trimEnd()}…` : message;
  }

  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  const spring = { duration: SLIDE, easing: cubicOut };

  let copied = "";
  let saved = "";
  const imageCache = new Map<string, Promise<string | null>>();

  $: items = buildItems(events);
  // Large sessions (hundreds of steps/messages) render the recent tail first so
  // switching sessions stays instant; older history loads on demand.
  let openFolds = new Set<string>();
  const WINDOW_STEP = 80;
  let visibleCount = WINDOW_STEP;
  let prevSession = "";
  $: if (session !== prevSession) {
    prevSession = session;
    visibleCount = WINDOW_STEP;
    openFolds = new Set();
  }
  $: windowStart = (() => {
    if (items.length <= visibleCount + 12) return 0;
    let start = items.length - visibleCount;
    // Align backward to the start of a turn so we never split mid-fold.
    while (start > 0 && items[start].kind !== "user") start--;
    return start;
  })();
  $: shownItems = windowStart > 0 ? items.slice(windowStart) : items;
  function showEarlier() {
    visibleCount += WINDOW_STEP;
  }

  $: folds = foldTurns(items, streaming ? { steps: liveTools.length } : null);
  // The running turn's clock, from its message, or from when this window
  // saw the run start.
  let since = Date.now();
  let now = since;
  let tick: ReturnType<typeof setInterval> | undefined;
  let clockFor = "";
  $: if (streaming && !tick) {
    since = now = Date.now();
    tick = setInterval(() => (now = Date.now()), 1000);
  } else if (!streaming && tick) {
    clearInterval(tick);
    tick = undefined;
  }
  $: if (session !== clockFor) {
    clockFor = session;
    since = now = Date.now();
  }
  onDestroy(() => clearInterval(tick));
  const liveSecs = (start: number, at: number) => (at - (start ? start * 1000 : since)) / 1000;
  $: liveWork = !!liveReasoning || liveTools.length > 0;

  function toggleFold(key: string) {
    openFolds = new Set(openFolds);
    if (openFolds.has(key)) openFolds.delete(key);
    else openFolds.add(key);
  }
  const shut = (key: string, open: Set<string>) => {
    const f = folds.inside.get(key);
    return !!f && !open.has(f);
  };
  $: turnEnds = (() => {
    const ends = new Set<string>();
    let last: string | null = null;
    for (const it of items) {
      if (it.kind === "assistant") last = it.key;
      else if (it.kind === "user") {
        if (last) ends.add(last);
        last = null;
      }
    }
    if (last && !streaming) ends.add(last);
    return ends;
  })();
  $: newest = (() => {
    const m = new Map<string, number>();
    for (const it of items) if (it.kind === "artifact") m.set(it.id, Math.max(m.get(it.id) ?? 0, it.version ?? 1));
    return m;
  })();
  $: userNth = (() => {
    const m = new Map<string, number>();
    let n = 0;
    events.forEach((e, i) => {
      if (e.kind === "user") m.set(`e${i}`, n++);
    });
    return m;
  })();
  $: replyAt = (() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it.kind === "user") return -1;
      if (it.kind === "widget" && it.fence === "parzi-replies") return i;
    }
    return -1;
  })();
  $: lastError = items.length && items[items.length - 1].kind === "error" ? items[items.length - 1].key : undefined;
  $: fix = provider ? nextStep($board.find((b) => b.provider === provider)) : null;
  $: fixWait = provider ? $setupWaits.get(provider) : undefined;
  $: liveSegs = liveText ? splitSegments(liveText, false) : [];
  $: tailIdx = liveSegs.map((s) => s.kind).lastIndexOf("md");
  // Only the block being written is rendered again each frame; the page
  // patches just the blocks that changed (htmlBlocks).
  $: liveBlockHtml = tailIdx >= 0 ? liveHtml(String(liveSegs[tailIdx].body), $richReady) : [];

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

  /** The command a shell step ran, whichever agent's shell it was. */
  function shellCommand(name: string, args: unknown): string {
    if (!/^(shell\.exec|shell_exec|mcp__parzi__shell_exec|execute|bash|Bash|shell)$/.test(name)) return "";
    return arg(args, "cmd") || arg(args, "command");
  }

  /** The last finished test run among a group's steps. */
  function lastTestRun(tools: Step[]): { run: TestRun; command: string } | null {
    for (let i = tools.length - 1; i >= 0; i--) {
      const t = tools[i];
      if (t.running || !t.output) continue;
      const run = testRunOf(t.output);
      if (run) return { run, command: t.cmd ?? "" };
    }
    return null;
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
      case "models.list":
        return "Listing models";
      case "ask.user":
        return `Asking ${arg(args, "question") || "a question"}`;
      case "plan.write":
        return "Writing the plan";
      case "plan.read":
        return "Reading the plan";
      case "session.rename":
        return arg(args, "title") ? `Renaming the session to ${arg(args, "title")}` : "Renaming the session";
      case "session.spawn":
        return `Starting a helper: ${arg(args, "title") || "untitled"}`;
      case "session.send":
      case "session.send_message":
        return `Messaging ${arg(args, "title", "session", "session_id") || "a helper"}`;
      case "session.read":
      case "session.read_session":
        return `Checking on ${arg(args, "title", "session", "session_id") || "a helper"}`;
      case "session.list":
        return "Listing helpers";
      case "project.create":
        return `Creating ${arg(args, "title") || "a project"}`;
      case "project.archive":
        return `Archiving ${arg(args, "slug") || "a project"}`;
      case "project.delete":
        return `Deleting ${arg(args, "slug") || "a project"}`;
      case "shell.start":
        return `Starting ${arg(args, "cmd") || "a command"}`;
      case "shell.logs":
        return "Reading command output";
      case "shell.kill":
        return "Stopping a command";
      case "wait":
        return "Waiting for background work";
      case "browser.click":
        return `Clicking ${arg(args, "text", "selector") || "a control"}`;
      case "browser.type":
        return `Typing into ${arg(args, "field", "selector") || "a field"}`;
      case "brain.search":
        return `Searching notes for ${arg(args, "query") || "?"}`;
      case "brain.read":
        return `Reading note ${arg(args, "path") || "?"}`;
      case "brain.write":
        return `Writing note ${arg(args, "path") || "?"}`;
      case "brain.delete":
        return `Deleting note ${arg(args, "path") || "?"}`;
      case "ui.show_artifact":
        return `Publishing ${arg(args, "title", "id") || "an artifact"}`;
      default: {
        const short = (name.split(/__|\./).pop() || name).replace(/_/g, " ");
        const hint = arg(args, "path", "file", "cmd", "query", "url", "title", "id");
        return hint ? `Using ${short}: ${hint}` : `Using ${short}`;
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
        steps(key).tools.push({ id: e.id, label: toolLabel(e.name, e.args), running: !r, cmd: shellCommand(e.name, e.args), ...r });
        return;
      }
      if (e.kind === "reasoning") {
        const g = steps(key);
        g.reasoning = g.reasoning ? `${g.reasoning}

${e.text}` : e.text;
        return;
      }
      // Notices, compactions and messages from other sessions are in the
      // Running card; old plan, task and tidy cards are not shown.
      if (e.kind === "tool_result" || e.kind === "checkpoint" || (e.kind === "widget" && UNSHOWN.has(e.fence))) return;
      if (e.kind === "system" && !isStop(e.text)) return;
      group = null;
      if (e.kind === "system") out.push({ key, kind: "stop", at: e.at });
      else if (e.kind === "error") out.push({ key, kind: "error", class: e.class, message: e.message, at: e.at });
      else if (e.kind === "widget") out.push({ key, kind: "widget", fence: e.fence, payload: e.payload });
      else if (e.kind === "artifact") out.push({ key, ...e });
      else if (e.kind === "assistant" && echoLine(e.text)) out.push({ key, kind: "echo", text: echoLine(e.text), at: e.at });
      else out.push({ key, kind: e.kind, text: e.text, at: e.at });
    });
    return out;
  }
  const UNSHOWN = new Set(["parzi-todo", "parzi-task", "parzi-tidy"]);

  function attachedImages(text: string): string[] {
    const m = /\[attached: ([^\]]+)\]/.exec(text);
    return m ? m[1].split(",").map((s) => s.trim()).filter((s) => IMAGE.test(s)) : [];
  }

  function isStop(text: string): boolean {
    return /^stopped$/i.test(text.trim());
  }

  function stripMarker(text: string): string {
    return text
      .replace(/\n?\[attached: [^\]]+\]/, "")
      .replace(/\n*<side-task[^>]*>[\s\S]*?<\/side-task>\n*/, "")
      .trimEnd();
  }

  function sideFrom(text: string): string {
    return /<side-task from="([^"]*)">/.exec(text)?.[1] ?? "";
  }

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
      toast(`Saved to projects as ${note.title || note.path}`);
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

  async function respond(answer: string) {
    if (!question || !answer.trim()) return;
    const { key, session } = question;
    try {
      await api.answerQuestion(key, session, answer.trim());
    } catch (e) {
      toastError(e);
    } finally {
      dispatch("answered", { key, session, text: answer.trim() });
    }
  }

  let replyText = "";

  async function reply(text: string) {
    if (!request) return;
    const { key, session } = request;
    replyText = "";
    try {
      await api.answerQuestion(key, session, text.trim());
    } catch (e) {
      toastError(e);
    } finally {
      dispatch("answered", { key });
    }
  }

  async function chooseFile() {
    const picture = request?.kind === "image";
    try {
      const path = await open({
        multiple: false,
        directory: false,
        defaultPath: folder || undefined,
        filters: picture ? [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif"] }] : undefined,
      });
      if (typeof path === "string" && path) await reply(path);
    } catch (e) {
      toastError(e);
    }
  }

  async function onClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    if (el instanceof HTMLImageElement && el.closest(".sent, .msg") && !el.closest("a")) {
      viewImage(el.currentSrc || el.src, el.alt);
      return;
    }
    if (await handleLinkClick(e, folder)) return;
    codeBlockClick(el);
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
<div class="thread-col" on:click={onClick}>
  {#if draft}
    <DraftCard title={draft.title} brief={draft.brief} starting={draftStarting} on:start={() => dispatch("startDraft")} on:remove={() => dispatch("removeDraft")} />
  {/if}
  {#if windowStart > 0}
    <button class="earlier" on:click={showEarlier}>
      Show earlier messages ({windowStart} hidden)
    </button>
  {/if}
  {#each shownItems as item (item.key)}
    {@const fold = folds.before.get(item.key)}
    {@const hidden = shut(item.key, openFolds)}
    {@const folded = folds.inside.has(item.key)}
    {#if fold}
      {@const canOpen = fold.work || (fold.live && liveWork)}
      <button class="fold" class:live={fold.live} disabled={!canOpen} aria-expanded={canOpen ? openFolds.has(fold.key) : undefined} on:click={() => toggleFold(fold.key)}>
        <span class="fold-text">{turnLabel(fold, liveSecs(fold.start, now))}</span>
        {#if canOpen}<span class="chev" class:open={openFolds.has(fold.key)}><Icon name="chevDown" size={12} /></span>{/if}
      </button>
    {/if}
    {#if !hidden || item.kind === "steps"}
      {#if item.kind === "user"}
        {@const images = attachedImages(item.text)}
        {@const said = splitQuotes(stripMarker(item.text))}
        {@const from = sideFrom(item.text)}
        <div class="row user">
          {#if from}<span class="side-from">Side session from {from}</span>{/if}
          <div class="user-line">
            {#if when(item.at)}<span class="when">{when(item.at)}</span>{/if}
            {#if said.text}
              <button class="copy" use:tooltip={"Copy"} on:click={() => copy(said.text, item.key)}>
                <Icon name={copied === item.key ? "check" : "copy"} />
              </button>
            {/if}
            <button class="copy" use:tooltip={"Edit in composer"} on:click={() => dispatch("edited", { text: said.text })}>
              <Icon name="pencil" />
            </button>
            {#if userNth.has(item.key)}
              <button
                class="copy"
                use:tooltip={"Rewind: go on from before this message"}
                on:click={() => dispatch("rewind", { nth: userNth.get(item.key) ?? 0, text: said.text })}
              >
                <Icon name="rewind" />
              </button>
            {/if}
            {#if said.text}
              <div class="bubble msg">{@html mdHtml(said.text, $richReady)}</div>
            {:else}
              <div class="quotes">{#each said.titles as t, i (i)}<span class="quote"><Icon name="plan" size={12} /><span class="q-name">{t}</span></span>{/each}</div>
            {/if}
          </div>
          {#if said.text && said.titles.length}
            <div class="quotes">{#each said.titles as t, i (i)}<span class="quote"><Icon name="plan" size={12} /><span class="q-name">{t}</span></span>{/each}</div>
          {/if}
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
        <div class="row" class:in-fold={folded}>
          <div class="body">
            {#each splitSegments(item.text) as seg}
              {#if seg.kind === "md"}
                <div class="msg">{@html mdHtml(seg.body, $richReady)}</div>
              {:else if seg.kind === "widget"}
                <Widget data={seg.body} />
              {:else if seg.kind === "artifact"}
                <ArtifactCard data={seg.body} inline />
              {:else if seg.kind === "viz"}
                <Viz spec={seg.body} />
              {/if}
            {/each}
          </div>
          {#if turnEnds.has(item.key)}
          <div class="actions">
            {#if when(item.at)}<span class="when">{when(item.at)}</span>{/if}
            <button class="copy" use:tooltip={"Copy"} on:click={() => copy(item.text, item.key)}>
              <Icon name={copied === item.key ? "check" : "copy"} />
            </button>
            <button class="copy" use:tooltip={"Save to projects"} on:click={() => save(item.text, item.key)}>
              <Icon name={saved === item.key ? "check" : "project"} />
            </button>
            <button class="copy" use:tooltip={"Fork: go on in a copy of this thread"} on:click={() => dispatch("forked")}>
              <Icon name="fork" />
            </button>
          </div>
          {/if}
        </div>
      {:else if item.kind === "steps"}
        {#if !hidden}
          {@const tests = lastTestRun(item.tools)}
          <div class="work in-fold">
            <Steps reasoning={item.reasoning} tools={item.tools} />
            {#if tests}<TestCard run={tests.run} command={tests.command} />{/if}
          </div>
        {/if}
        <ToolPictures {session} paths={picturesIn(item.tools.map((t) => t.output ?? ""))} />
      {:else if item.kind === "echo"}
        <div class="echo in-fold">{item.text}</div>
      {:else if item.kind === "error"}
        {@const reset = item.class === "rate_limit" ? resetLine(provider, $board) : ""}
        <div class="turn-error" role="alert" use:tooltip={shortErr(item.message)}>
          <span class="err-dot" aria-hidden="true" />
          <span class="err-class">{ERROR_WORDS[item.class] ?? ERROR_WORDS.unknown}</span>
          <span class="err-msg">{reset ? reset.replace(/\.$/, "") : shortErr(item.message)}</span>
          {#if item.key === lastError && !streaming}
            {#if fixWait}
              <span class="err-msg">{fixWait.note}</span>
            {:else if fix}
              <button class="btn sm" on:click={() => fix?.run(provider)}>{fix.label}</button>
            {:else if item.class === "context_overflow"}
              <button class="btn sm" on:click={() => dispatch("compact")}>Compact</button>
            {:else if item.class === "rate_limit"}
              <button class="btn sm primary" on:click={() => dispatch("pickModel")}>Switch model</button>
              <button class="btn sm" on:click={() => dispatch("retry")}>Try again</button>
            {:else if RETRYABLE.has(item.class)}
              <button class="btn sm" on:click={() => dispatch("retry")}>Try again</button>
            {/if}
          {/if}
        </div>
      {:else if item.kind === "widget"}
        {#if item.fence === "parzi-widget"}
          <Widget data={item.payload} />
        {:else if item.fence === "viz"}
          <Viz spec={item.payload} />
        {:else if item.fence !== "parzi-replies"}
          <div class="msg">{@html mdHtml(fenced(JSON.stringify(item.payload, null, 2), item.fence), $richReady)}</div>
        {/if}
      {:else if item.kind === "artifact"}
        {#if (item.version ?? 1) >= (newest.get(item.id) ?? 0)}
          <ArtifactCard data={item.payload} fallbackId={item.id} fallbackTitle={item.title} fallbackKind={item.artifact_kind} inline />
        {/if}
      {/if}
    {/if}
  {/each}

  {#if folds.tail}
    {@const tail = folds.tail}
    {@const canOpen = tail.work || (tail.live && liveWork)}
    <button class="fold" class:live={tail.live} disabled={!canOpen} aria-expanded={canOpen ? openFolds.has(tail.key) : undefined} on:click={() => toggleFold(tail.key)}>
      <span class="fold-text">{turnLabel(tail, liveSecs(tail.start, now))}</span>
      {#if canOpen}<span class="chev" class:open={openFolds.has(tail.key)}><Icon name="chevDown" size={12} /></span>{/if}
    </button>
  {/if}

  {#if liveWork && folds.live && openFolds.has(folds.live.key)}
    <div class="in-fold" transition:fade|local={spring}>
      <Steps reasoning={liveReasoning} tools={liveTools} />
    </div>
  {/if}

  <!-- Laid out exactly as the saved message that replaces it at the end of
       the turn, so the swap moves nothing. -->
  {#if liveSegs.length}
    <div class="row">
      <div class="body">
        {#each liveSegs as seg, i}
          {#if seg.kind === "md" && i === tailIdx}
            <div class="msg" use:htmlBlocks={liveBlockHtml}></div>
          {:else if seg.kind === "md"}
            <div class="msg">{@html mdHtml(seg.body, $richReady)}</div>
          {:else if seg.kind === "widget"}
            <Widget data={seg.body} />
          {:else if seg.kind === "artifact"}
            <ArtifactCard data={seg.body} inline />
          {:else if seg.kind === "viz"}
            <Viz spec={seg.body} />
          {/if}
        {/each}
      </div>
    </div>
  {/if}

  {#each liveArtifacts.filter((a) => a.version > (newest.get(a.id) ?? 0)) as a (a.id)}
    {#key a.version}
      <ArtifactCard data={a.payload} fallbackId={a.id} fallbackTitle={a.title} fallbackKind={a.artifact_kind} inline />
    {/key}
  {/each}

  {#if !streaming && replyAt >= 0}
    {@const offer = items[replyAt]}
    {#if offer.kind === "widget"}
      <ReplyButtons payload={offer.payload} on:reply={(e) => dispatch("reply", e.detail)} />
    {/if}
  {/if}
  <slot />

  {#if approval}
    {@const call = approval.call}
    {@const who = approval.session === session ? agentName || "The agent" : askerTitle || "Another session"}
    <div class="approval" role="group" aria-label="Approval" transition:fade={spring}>
      <div class="ask-line">
        <span class="who">{who}</span> wants to {call.action || (call.label ? call.label.charAt(0).toLowerCase() + call.label.slice(1) : `use ${call.name}`)}
      </div>
      {#if call.detail}
        <div class="ask-detail" class:mono={call.detail_kind === "command" || call.detail_kind === "paths"}>{call.detail}</div>
      {/if}
      {#if call.note}<div class="ask-note">{call.note}</div>{/if}
      {#if rawOpen}
        <pre class="ask-raw">{call.name} {JSON.stringify(call.args, null, 2)?.slice(0, 2000)}</pre>
      {/if}
      <div class="approval-actions">
        <button class="btn primary" on:click={() => vote(true)}>Allow</button>
        <button class="btn" on:click={() => vote(false)}>Deny</button>
        <button class="raw-toggle" aria-expanded={rawOpen} on:click={() => (rawOpen = !rawOpen)}>{rawOpen ? "Hide details" : "Details"}</button>
      </div>
    </div>
  {/if}

  {#if question}
    <div transition:fade={spring}>
      <QuestionCard {question} on:answer={(e) => respond(e.detail.text)} />
    </div>
  {:else if proposal}
    <!-- A proposed plan waits on the session, not on a tool call: the
         window answers it and sends the answer as the next message. -->
    <div transition:fade={spring}>
      <QuestionCard {proposal} on:accept={() => dispatch("proposal", { session })} />
    </div>
  {/if}

  {#if request}
    {@const wantsFile = request.kind === "file" || request.kind === "image"}
    <div class="approval ask" transition:fade={spring}>
      {#if askerTitle}<div class="ask-from">{askerTitle} {request.kind === "file" ? "needs a file" : request.kind === "image" ? "needs a picture" : "needs something from you"}</div>{/if}
      <div class="ask-q">{request.request}</div>
      <div class="ask-free">
        <input
          aria-label="Reply"
          placeholder={wantsFile ? "Paste a path… (Enter sends)" : "Type your reply… (Enter sends)"}
          bind:value={replyText}
          on:keydown={(e) => {
            if (e.key === "Enter" && replyText.trim()) reply(replyText);
          }}
        />
        {#if wantsFile}<button class="btn" on:click={chooseFile}>Choose file</button>{/if}
        <button class="btn primary" disabled={!replyText.trim()} on:click={() => reply(replyText)}>Send</button>
        <button class="btn quiet" on:click={() => reply("")}>Decline</button>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Room to read: blocks in a turn sit 16px apart, and each of the
     person's messages opens a new turn with a clear break above it. */
  .thread-col {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 4);
    width: 100%;
    max-width: calc(var(--read) + 48px);
    margin: 0 auto;
    padding: calc(var(--space) * 8) calc(var(--space) * 6) calc(var(--space) * 2);
  }
  .row {
    display: flex;
    flex-direction: column;
  }
  .row.user {
    align-items: flex-end;
  }
  .row.user:not(:first-child) {
    margin-top: calc(var(--space) * 8);
  }
  .fold {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: var(--h-sm);
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .fold:hover:not(:disabled) {
    color: var(--text);
  }
  .fold:disabled {
    cursor: default;
  }
  .fold.live .fold-text {
    animation: breathe 1.6s ease-in-out infinite;
  }
  .fold .chev {
    display: inline-flex;
    transform: rotate(-90deg);
    transition: transform var(--dur) ease;
  }
  .fold .chev.open {
    transform: none;
  }
  .in-fold {
    margin-left: var(--space);
    padding-left: calc(var(--space) * 4);
    border-left: 1px solid var(--line);
  }
  .work {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
  }
  .echo {
    font-family: var(--mono);
    font-size: var(--fs-sm);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .side-from {
    margin-bottom: var(--space);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .user-line {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
    max-width: 85%;
  }
  .bubble {
    min-width: 0;
    max-width: 100%;
    padding: calc(var(--space) * 4);
    background: var(--surface);
    border: 1px solid var(--edge);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-base);
    line-height: 1.5;
    overflow-wrap: break-word;
  }
  .bubble:has(p + p, pre, ul, ol, blockquote) {
    border-radius: var(--radius);
  }
  .bubble :global(h1),
  .bubble :global(h2),
  .bubble :global(h3) {
    margin: 0.5em 0 0.3em;
    font-size: var(--fs-base);
  }
  .bubble :global(ul),
  .bubble :global(ol) {
    margin: 0.4em 0;
    padding-left: calc(var(--space) * 6);
  }

  .quotes {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
    min-width: 0;
    max-width: 85%;
  }
  .user-line .quotes {
    max-width: none;
  }
  .row.user > .quotes {
    margin-top: calc(var(--space) * 2);
  }
  .quote {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
    max-width: 280px;
    height: 26px;
    padding: 0 calc(var(--space) * 3);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--hover);
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .q-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .sent {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: calc(var(--space) * 2);
    margin-top: calc(var(--space) * 2);
  }

  .sent img {
    cursor: zoom-in;
    height: 88px;
    width: auto;
    min-width: 88px;
    max-width: 200px;
    object-fit: cover;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    color: color-mix(in srgb, var(--text) 92%, transparent);
  }
  .actions {
    height: var(--h-sm);
    margin-left: calc(var(--space) * -1);
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  .copy {
    width: 24px;
    height: var(--h-sm);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--dur) ease;
  }
  .row:hover .copy,
  .row:focus-within .copy {
    opacity: 1;
  }
  .when {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    opacity: 0;
    transition: opacity var(--dur) ease;
    cursor: default;
  }
  .row:hover .when,
  .row:focus-within .when {
    opacity: 1;
  }
  .copy:hover {
    background: var(--line);
    color: var(--text);
  }
  /* A failed turn is one line: what went wrong, why in short, and the
     button that goes on. The whole message is in its tooltip. */
  .turn-error {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
    min-height: var(--h-sm);
    font-size: var(--fs-sm);
  }
  .err-dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--bad);
  }
  .err-class {
    flex: none;
    font-weight: 500;
  }
  .err-msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .turn-error .btn.sm {
    flex: none;
    align-self: auto;
  }

  .approval {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
    padding: calc(var(--space) * 3) calc(var(--space) * 4);
    background: var(--surface);
    border: 1px solid var(--edge);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
    font-size: var(--fs-md);
    color: var(--text);
  }
  .ask-line {
    color: var(--muted);
  }
  .ask-line .who {
    color: var(--text);
    font-weight: 500;
  }
  .ask-detail {
    max-height: 7.5em;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    overflow: auto;
    background: var(--fill);
    border-radius: var(--radius);
    color: var(--text);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.5;
  }
  .ask-detail.mono {
    font-family: var(--mono);
    font-size: var(--fs-sm);
  }
  .ask-note {
    font-size: var(--fs-sm);
    color: var(--warn-text);
  }
  .ask-raw {
    max-height: 160px;
    margin: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    overflow: auto;
    background: var(--fill);
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .approval-actions {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
  }
  .raw-toggle {
    margin-left: auto;
    padding: var(--space) calc(var(--space) * 2);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .raw-toggle:hover {
    color: var(--text);
  }
  .ask-q {
    font-weight: 600;
  }
  .ask-from {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .ask-free {
    display: flex;
    gap: calc(var(--space) * 2);
  }
  .ask-free input {
    flex: 1;
    min-width: 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 3);
    background: var(--fill);
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  .ask-free input:focus {
    border-color: var(--accent);
  }
  .ask-free .btn {
    flex: none;
  }
  .btn.quiet {
    background: none;
    border-color: transparent;
    color: var(--muted);
  }
  .btn.quiet:hover:not(:disabled) {
    background: var(--line);
    border-color: transparent;
    color: var(--text);
  }
  .earlier {
    align-self: center;
    margin: calc(var(--space) * 1.5) 0;
    padding: var(--space) calc(var(--space) * 3);
    background: var(--fill);
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .earlier:hover {
    color: var(--text);
    background: var(--fill-hover);
  }
</style>
