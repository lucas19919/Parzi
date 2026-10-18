<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { handleLinkClick } from "./links";
  import { mdHtml, richReady, splitSegments, LiveMarkdown } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import Viz from "./viz/Viz.svelte";
  import Steps from "./Steps.svelte";
  import Icon from "./Icon.svelte";
  import { api, type AgentRequest, type ChatEvent, type PlanQuote, type Question } from "./api";
  import type { Approval, LiveTool, Step } from "./live";
  import { splitQuotes } from "./planModel";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";
  import { formatTime } from "./time";
  import { viewImage } from "./imageViewer";
  import ReplyButtons from "./ReplyButtons.svelte";
  import TaskCard from "./TaskCard.svelte";
  import TodoList from "./TodoList.svelte";
  import QuestionCard from "./QuestionCard.svelte";
  import ToolPictures, { picturesIn } from "./ToolPictures.svelte";
  import LiveLine from "./LiveLine.svelte";
  import { board, nextStep, setupWaits } from "./providerStore";

  // What the waiting prompt answers: the nearest assistant message above
  // it, so "the proposal above" names what was actually said. Nothing
  // when the agent asked without saying anything first.
  $: askContext = (() => {
    for (let i = events.length - 1; i >= 0; i--) {
      const e = events[i];
      if ((e.kind === "assistant" || e.kind === "user") && e.text.trim()) {
        const line = e.text.trim().split("\n")[0];
        return line.length > 140 ? `${line.slice(0, 140)}…` : line;
      }
    }
    return "";
  })();
  function when(at: number | undefined): string {
    if (!at) return "";
    return formatTime(at * 1000);
  }

  function whenTitle(at: number | undefined): string {
    if (!at) return "";
    const d = new Date(at * 1000);
    return d.toLocaleString(undefined, { weekday: "short", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }

  export let events: ChatEvent[] = [];
  export let liveText = "";
  export let liveReasoning = "";
  export let liveTools: LiveTool[] = [];
  export let streaming = false;
  // Whose thread: pictures its tools made are read by it.
  export let session = "";
  // The checklist todo.write just set, ahead of the saved one.
  export let liveTodo: unknown = null;
  export let approval: Approval | null = null;
  export let question: Question | null = null;
  // Something the agent needs from you: text, a file or a picture.
  export let request: AgentRequest | null = null;
  // Who is asking: the session's title, so a question or approval reads
  // with its context instead of floating anonymous in the thread.
  export let askerTitle = "";
  // This thread's agent by name ("Grok"): an approval of its own reads
  // "Grok wants to …".
  export let agentName = "";
  let rawOpen = false;
  $: if (!approval) rawOpen = false;
  export let folder = "";
  // The agent this thread runs on, so a failed turn can offer its fix.
  export let provider = "";
  // Names another session, for messages and reports that come from one.
  export let sessionTitle: (id: string) => string = () => "Another session";

  const dispatch = createEventDispatcher<{
    voted: { key: string };
    answered: { key: string };
    edited: { text: string; quotes: PlanQuote[] };
    forked: void;
    retry: void;
    compact: void;
    reply: { text: string };
    startTask: { task: { id: string; title: string; brief: string; project?: string }; done: (session: string | null) => void };
    openSession: { id: string };
    rewind: { nth: number; text: string; quotes: PlanQuote[] };
    pickModel: void;
  }>();

  // When the agent's spent limit comes back, from its usage windows: the
  // soonest reset of one at 100%, else of the fullest one.
  function resetLine(p: string): string {
    const windows = $board.find((b) => b.provider === p)?.usage ?? [];
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
    overloaded: "The service is overloaded. Wait a bit, then send again",
    context_overflow: "The conversation is too long",
    bad_request: "The request didn't go through",
    process: "The agent's program stopped",
    session_lost: "The agent lost this conversation",
    unknown: "The turn failed",
  };
  // Errors a plain second try can get past.
  const RETRYABLE = new Set(["rate_limit", "overloaded", "process", "session_lost", "unknown"]);
  // "Conversation compacted · 92k → 12k tokens", with what is known.
  function compactedTitle(before?: number, after?: number): string {
    const k = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);
    if (before && after) return `Conversation compacted · ${k(before)} → ${k(after)} tokens`;
    if (before) return `Conversation compacted · was ${k(before)} tokens`;
    return "Conversation compacted";
  }
  const ERROR_HINT: Record<string, string> = {
    rate_limit: "Your message is kept. Nothing was lost.",
  };

  // Vendor errors often trail a raw JSON blob; cut it, it never helped.
  function shortErr(message: string): string {
    const cut = message.search(/\s*\{[\s"]*"errorName"|\s*\{"errorName"|\s*\{"code"/);
    if (cut > 0) return `${message.slice(0, cut).trimEnd()}…`;
    return message.length > 400 ? `${message.slice(0, 400).trimEnd()}…` : message;
  }

  // The first line reads as the reason; the rest (stderr, stack) waits
  // behind Details.
  function errLead(message: string): { lead: string; more: string } {
    const text = shortErr(message);
    const nl = text.indexOf("\n");
    const end = nl > 0 && nl <= 160 ? nl : Math.min(text.length, 160);
    if (end >= text.length) return { lead: text, more: "" };
    return { lead: nl === end ? text.slice(0, end) : `${text.slice(0, end).trimEnd()}…`, more: text };
  }
  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;
  const spring = { duration: 160, easing: cubicOut };

  type Item =
    | { key: string; kind: "user" | "assistant" | "system"; text: string; at?: number }
    | { key: string; kind: "checkpoint"; text: string; at?: number; before?: number; after?: number }
    | { key: string; kind: "error"; class: string; message: string; at?: number }
    | { key: string; kind: "inter"; from: string; text: string; at?: number }
    | { key: string; kind: "widget"; fence: string; payload: unknown }
    | { key: string; kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown }
    | { key: string; kind: "steps"; reasoning: string; tools: Step[] };

  let copied = "";
  let saved = "";
  const liveMd = new LiveMarkdown();
  const imageCache = new Map<string, Promise<string | null>>();

  $: items = buildItems(events);
  // A turn's last reply carries its actions (copy, save, fork); the replies
  // before it in the same turn stay close together, as they read.
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
  // Each artifact's newest version: older ones fold to a line.
  $: newest = (() => {
    const m = new Map<string, number>();
    for (const it of items) if (it.kind === "artifact") m.set(it.id, Math.max(m.get(it.id) ?? 0, it.version ?? 1));
    return m;
  })();
  // Each of the person's messages by its place among them, as the store
  // counts them for a rewind.
  $: userNth = (() => {
    const m = new Map<string, number>();
    let n = 0;
    events.forEach((e, i) => {
      if (e.kind === "user") m.set(`e${i}`, n++);
    });
    return m;
  })();
  // The session's checklist: the last one its agent wrote.
  $: todo =
    liveTodo ??
    [...items].reverse().find((i): i is Extract<typeof i, { kind: "widget" }> => i.kind === "widget" && i.fence === "parzi-todo")?.payload ??
    null;
  // Offered replies stand only until the next message: the latest offer,
  // with nothing said after it.
  $: replyAt = (() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it.kind === "user") return -1;
      if (it.kind === "widget" && it.fence === "parzi-replies") return i;
    }
    return -1;
  })();
  // Only the newest error offers a fix, and only while the agent still
  // needs it: once signed in, the card is history.
  $: lastError = [...items].reverse().find((i) => i.kind === "error")?.key;
  $: fix = provider ? nextStep($board.find((b) => b.provider === provider)) : null;
  $: fixWait = provider ? $setupWaits.get(provider) : undefined;
  $: running = liveTools.filter((t) => t.running);
  // A waiting prompt beats the run status: "Starting…" while an answer sits
  // open is exactly the confusion this guards.
  $: status = approval
    ? "Needs your approval"
    : question || request
      ? "Waiting for your answer"
      : !streaming
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
  // The live line under the thread says it plainly: a running tool, else
  // the checklist's active item, else thinking or writing.
  $: activeTask = (((todo ?? {}) as { items?: { title: string; state: string }[] }).items ?? []).find((i) => i.state === "active")?.title ?? "";
  $: lineText =
    status && (approval || question || request || running.length)
      ? status
      : activeTask
        ? `${activeTask}…`
        : liveText
          ? "Writing…"
          : status || "Working…";
  $: lineTokens = Math.round((liveText.length + liveReasoning.length) / 4);
  $: showLiveSteps = !!liveReasoning || liveTools.length > 0;
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
        return "Listing models";
      case "ask.user":
        return `Asking ${arg(args, "question") || "a question"}`;
      case "request.user":
        return `Requesting ${arg(args, "request") || "something"}`;
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
      case "lane.dispatch": {
        const lane = arg(args, "lane");
        return lane ? `Handing off to ${lane[0].toUpperCase()}${lane.slice(1)}` : "Handing off";
      }
      case "lane.status":
        return "Checking what is running";
      case "memory.review":
        return `Reviewing note ${arg(args, "path") || "?"}`;
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
        return "Writing a note";
      case "ui.show_artifact":
        return `Publishing ${arg(args, "title", "id") || "an artifact"}`;
      default: {
        // Vendor and connector tools ("mcp__github__list_issues") read by
        // their last word.
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
      if (e.kind === "checkpoint") out.push({ key, kind: "checkpoint", text: e.summary, at: e.at, before: e.before, after: e.after });
      else if (e.kind === "error") out.push({ key, kind: "error", class: e.class, message: e.message, at: e.at });
      else if (e.kind === "widget") out.push({ key, kind: "widget", fence: e.fence, payload: e.payload });
      else if (e.kind === "artifact") out.push({ key, ...e });
      else if (e.kind === "assistant" && echoLine(e.text)) {
        pushSys(key, out, echoLine(e.text), e.at);
      } else if (e.kind === "system" && interOf(e.text)) {
        const m = interOf(e.text)!;
        out.push({ key, kind: "inter", from: m.from, text: m.body, at: e.at });
      } else if (e.kind === "system") {
        pushSys(key, out, e.text, e.at);
      } else out.push({ key, kind: e.kind, text: e.text, at: e.at });
    });
    return out;
  }

  // Consecutive routine lines join into one whisper instead of a stack.
  function pushSys(key: string, out: Item[], text: string, at?: number) {
    const prev = out[out.length - 1];
    if (prev?.kind === "system" && sysTone(prev.text) === "line" && sysTone(text) === "line") {
      prev.text = `${prev.text} · ${text.trim()}`;
      return;
    }
    out.push({ key, kind: "system", text, at });
  }

  // Another session's message or end-of-run report, stored as
  // `parzi-inter:1 {json}` for the agent: shown as words, not the wire format.
  function interOf(text: string): { from: string; body: string } | null {
    if (!text.startsWith("parzi-inter:1")) return null;
    try {
      const m = JSON.parse(text.slice("parzi-inter:1".length));
      return { from: sessionTitle(String(m.from_run ?? "")), body: String(m.body ?? "") };
    } catch {
      return null;
    }
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
    return text
      .replace(/\n?\[attached: [^\]]+\]/, "")
      .replace(/\n*<side-task[^>]*>[\s\S]*?<\/side-task>\n*/, "")
      .trimEnd();
  }

  // The session a side task was started from, from its opening message.
  function sideFrom(text: string): string {
    return /<side-task from="([^"]*)">/.exec(text)?.[1] ?? "";
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

  async function respond(answer: string) {
    if (!question || !answer.trim()) return;
    const { key, session } = question;
    try {
      await api.answerQuestion(key, session, answer.trim());
    } catch (e) {
      toastError(e);
    } finally {
      dispatch("answered", { key });
    }
  }

  let replyText = "";

  // A blank reply declines: the agent goes on without it.
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

  // Choosing the file is the reply: its path goes back to the agent.
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
    // An image you sent or one in a reply opens big; one inside a link
    // stays a link.
    if (el instanceof HTMLImageElement && el.closest(".sent, .msg") && !el.closest("a")) {
      viewImage(el.currentSrc || el.src, el.alt);
      return;
    }
    if (await handleLinkClick(e, folder)) return;
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
      {@const said = splitQuotes(stripMarker(item.text))}
      {@const from = sideFrom(item.text)}
      <div class="row user">
        {#if from}<span class="side-from">Side task from {from}</span>{/if}
        <div class="user-line">
          {#if when(item.at)}<span class="when">{when(item.at)}</span>{/if}
          {#if said.text}
            <button class="copy" use:tooltip={"Copy"} on:click={() => copy(said.text, item.key)}>
              <Icon name={copied === item.key ? "check" : "copy"} />
            </button>
          {/if}
          <button class="copy" use:tooltip={"Edit in composer"} on:click={() => dispatch("edited", { text: said.text, quotes: said.quotes })}>
            <Icon name="pencil" />
          </button>
          {#if userNth.has(item.key)}
            <button
              class="copy"
              use:tooltip={"Rewind: go on from before this message"}
              on:click={() => dispatch("rewind", { nth: userNth.get(item.key) ?? 0, text: said.text, quotes: said.quotes })}
            >
              <Icon name="rewind" />
            </button>
          {/if}
          {#if said.text}
            <div class="bubble msg">{@html mdHtml(said.text, $richReady)}</div>
          {:else}
            <div class="quotes">{#each said.quotes as q, i (i)}<span class="quote"><Icon name="plan" size={12} /><span class="q-name">Plan · {q.title}</span></span>{/each}</div>
          {/if}
        </div>
        {#if said.text && said.quotes.length}
          <div class="quotes">{#each said.quotes as q, i (i)}<span class="quote"><Icon name="plan" size={12} /><span class="q-name">Plan · {q.title}</span></span>{/each}</div>
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
      <div class="row">
        <div class="body">
          {#each splitSegments(item.text) as seg}
            {#if seg.kind === "md"}
              <div class="msg">{@html mdHtml(seg.body, $richReady)}</div>
            {:else if seg.kind === "widget"}
              <Widget data={seg.body} />
            {:else if seg.kind === "artifact"}
              <ArtifactCard data={seg.body} />
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
          <button class="copy" use:tooltip={"Save to brain"} on:click={() => save(item.text, item.key)}>
            <Icon name={saved === item.key ? "check" : "brain"} />
          </button>
          <button class="copy" use:tooltip={"Fork: go on in a copy of this thread"} on:click={() => dispatch("forked")}>
            <Icon name="fork" />
          </button>
        </div>
        {/if}
      </div>
    {:else if item.kind === "steps"}
      <Steps reasoning={item.reasoning} tools={item.tools} />
      <ToolPictures {session} paths={picturesIn(item.tools.map((t) => t.output ?? ""))} />
    {:else if item.kind === "checkpoint"}
      <Steps title={compactedTitle(item.before, item.after)} reasoning={item.text} />
    {:else if item.kind === "system"}
      {#if sysTone(item.text) === "line"}
        <div class="sysline" title={whenTitle(item.at)}>{item.text.slice(0, 300)}</div>
      {:else}
        <div class="system" title={whenTitle(item.at)}>{item.text.slice(0, 300)}</div>
      {/if}
    {:else if item.kind === "inter"}
      <div class="inter" title={whenTitle(item.at)}>
        <span class="inter-from">From {item.from}</span>
        <span class="inter-body">{item.text.slice(0, 2000)}</span>
      </div>
    {:else if item.kind === "error"}
      {@const msg = errLead(item.message)}
      <div class="turn-error" role="alert" title={whenTitle(item.at)}>
        <span class="err-class">{ERROR_WORDS[item.class] ?? ERROR_WORDS.unknown}</span>
        <span class="err-msg">{msg.lead}</span>
        {#if msg.more}
          <details class="err-more"><summary>Details</summary><pre>{msg.more}</pre></details>
        {/if}
        {#if item.class === "rate_limit" && resetLine(provider)}<span class="err-hint">{resetLine(provider)}</span>{/if}
        {#if ERROR_HINT[item.class]}<span class="err-hint">{ERROR_HINT[item.class]}</span>{/if}
        {#if item.key === lastError && !streaming}
          {#if fix || fixWait}
            <div class="err-fix">
              {#if fixWait}
                <span class="err-hint">{fixWait.note}</span>
              {:else if fix}
                <button class="err-btn" on:click={() => fix?.run(provider)}>{fix.label}</button>
                <span class="err-hint">Then try again.</span>
              {/if}
            </div>
          {:else if item.class === "context_overflow"}
            <div class="err-fix"><button class="err-btn" on:click={() => dispatch("compact")}>Compact and go on</button></div>
          {:else if item.class === "context_overflow"}
            <span class="err-hint">Start a new session to go on.</span>
          {:else if item.class === "rate_limit"}
            <div class="err-fix">
              <button class="err-btn" on:click={() => dispatch("pickModel")}>Change model</button>
              <button class="err-btn" on:click={() => dispatch("retry")}>Try again</button>
            </div>
          {:else if RETRYABLE.has(item.class)}
            <div class="err-fix"><button class="err-btn" on:click={() => dispatch("retry")}>Try again</button></div>
          {/if}
        {/if}
      </div>
    {:else if item.kind === "widget"}
      {#if item.fence === "parzi-replies"}
        <!-- The latest offer is drawn last, after the checklist. -->
      {:else if item.fence === "parzi-todo"}
        <!-- Drawn once, as it stands now, at the end of the thread. -->
      {:else if item.fence === "parzi-task"}
        <TaskCard payload={item.payload} on:start={(e) => dispatch("startTask", e.detail)} on:open={(e) => dispatch("openSession", e.detail)} />
      {:else if item.fence === "parzi-widget"}
        <Widget data={item.payload} />
      {:else if item.fence === "viz"}
        <Viz spec={item.payload} />
      {:else}
        <div class="msg">{@html mdHtml("```" + item.fence + "\n" + JSON.stringify(item.payload, null, 2) + "\n```", $richReady)}</div>
      {/if}
    {:else if item.kind === "artifact"}
      <ArtifactCard
        data={item.payload}
        fallbackId={item.id}
        fallbackTitle={item.title}
        fallbackKind={item.artifact_kind}
        superseded={(item.version ?? 1) < (newest.get(item.id) ?? 0)}
      />
    {/if}
  {/each}

  {#if showLiveSteps}
    <div transition:fade|local={spring}>
      <Steps reasoning={liveReasoning} tools={liveTools} />
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
    {:else if seg.kind === "viz"}
      <Viz spec={seg.body} />
    {/if}
  {/each}

  <!-- The plan checklist, after the conversation and before what waits on you. -->
  {#if todo}<TodoList {todo} working={streaming} />{/if}
  {#if streaming}<LiveLine text={lineText} tokens={lineTokens} />{/if}
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
      <QuestionCard {question} from={askerTitle} about={askContext} on:answer={(e) => respond(e.detail.text)} />
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
  .thread-col {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
    max-width: calc(var(--read) + 48px);
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
  /* A new turn stands a little apart from the last one. */
  .row.user:not(:first-child) {
    margin-top: 14px;
  }
  .side-from {
    margin-bottom: 4px;
    font-size: var(--fs-xs);
    color: var(--muted);
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
    border-radius: var(--radius-lg);
    color: var(--text);
    font-size: var(--fs-base);
    line-height: 1.5;
    overflow-wrap: break-word;
  }
  .bubble:has(p + p, pre, ul, ol, blockquote) {
    border-radius: var(--radius-lg);
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
    padding-left: 20px;
  }
  /* Plan items the message quoted: the desk sends them as blocks after the
     text; they read as chips. */
  .quotes {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    min-width: 0;
    max-width: 85%;
  }
  .user-line .quotes {
    max-width: none;
  }
  .row.user > .quotes {
    margin-top: 8px;
  }
  .quote {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 280px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
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
    gap: 6px;
    margin-top: 6px;
  }
  /* One tile height, whatever the picture's shape: a row of them lines
     up. A click shows the whole picture. */
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
    gap: 8px;
    color: color-mix(in srgb, var(--text) 92%, transparent);
  }
  .actions {
    height: 24px;
    margin-left: -4px;
    display: flex;
    align-items: center;
    gap: 8px;
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
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .row:hover .copy,
  .copy:focus-visible {
    opacity: 1;
  }
  /* A message's clock time, only while its row is hovered. */
  .when {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    opacity: 0;
    transition: opacity 140ms ease;
    cursor: default;
  }
  .row:hover .when {
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
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .sysline {
    font-size: var(--fs-sm);
    color: var(--muted);
    padding-left: 2px;
  }
  .inter {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    font-size: var(--fs-sm);
  }
  .inter-from {
    font-weight: 600;
    color: var(--muted);
  }
  .inter-body {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .turn-error {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--bad);
    border-radius: var(--radius);
    font-size: var(--fs-sm);
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
    color: var(--muted);
    font-size: var(--fs-sm);
  }
  .err-more summary {
    cursor: pointer;
    color: var(--muted);
  }
  .err-more pre {
    max-height: 160px;
    margin: 4px 0 0;
    overflow: auto;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .err-fix {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }
  .err-btn {
    padding: 4px 12px;
    background: var(--line);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 500;
    cursor: pointer;
  }
  .err-btn:hover {
    border-color: var(--muted);
  }
  /* Quiet like the thread around it: the live line already says it waits. */
  .approval {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
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
    padding: 8px 10px;
    overflow: auto;
    background: var(--bg);
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
    color: var(--warn);
  }
  .ask-raw {
    max-height: 160px;
    margin: 0;
    padding: 8px 10px;
    overflow: auto;
    background: var(--bg);
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .approval-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .raw-toggle {
    margin-left: auto;
    padding: 4px 6px;
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
  @media (prefers-reduced-motion: reduce) {
    .live-head {
      opacity: 1;
    }
  }
</style>
