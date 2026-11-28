<script lang="ts">
  import { SLIDE } from "./motion";
  import { createEventDispatcher } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { handleLinkClick } from "./links";
  import { codeBlockClick, fenced, htmlBlocks, liveHtml, mdHtml, richReady, splitSegments } from "./md";
  import Widget from "./widgets/Widget.svelte";
  import ArtifactCard from "./widgets/ArtifactCard.svelte";
  import Viz from "./viz/Viz.svelte";
  import WorkGroup from "./WorkGroup.svelte";
  import WorkRun from "./WorkRun.svelte";
  import Thinking from "./Thinking.svelte";
  import Icon from "./Icon.svelte";
  import { api, type AgentRequest, type ChatEvent, type Proposal, type ProviderStatus, type Question } from "./api";
  import type { Approval } from "./live";
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
  import { buildTurns, lastTurnStart, replyOf, workedLine, type Block, type ToolItem, type Turn } from "./timeline";

  function when(at: number | undefined): string {
    if (!at) return "";
    return formatTime(at * 1000);
  }

  export let events: ChatEvent[] = [];
  /** What the running turn has sent that the store may not have yet, in
   *  the shape of saved events: one timeline draws both. */
  export let liveEvents: ChatEvent[] = [];
  export let streaming = false;
  export let session = "";
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
    reviseProposal: void;
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

  // The thread before the last turn is built once per read of the store;
  // the last turn, with what is arriving live, is built again as it grows.
  // Keys are event indexes, so a live block keeps its key once saved.
  let headFor: ChatEvent[] | null = null;
  let headTurns: Turn[] = [];
  $: cut = lastTurnStart(events);
  $: if (events !== headFor) {
    headFor = events;
    headTurns = buildTurns(events.slice(0, cut), 0);
  }
  $: tailTurns = buildTurns(liveEvents.length ? [...events.slice(cut), ...liveEvents] : events.slice(cut), cut);
  $: turns = [...headTurns, ...tailTurns];

  // Large sessions render the recent turns first so switching stays instant.
  const WINDOW = 40;
  let visibleTurns = WINDOW;
  let prevSession = "";
  $: if (session !== prevSession) {
    prevSession = session;
    visibleTurns = WINDOW;
  }
  $: firstShown = Math.max(0, turns.length - visibleTurns);
  $: shownTurns = firstShown > 0 ? turns.slice(firstShown) : turns;

  $: liveTurn = streaming && turns.length ? turns[turns.length - 1] : null;
  /** The running turn shows nothing at work: say it is thinking. */
  $: waiting = !!liveTurn && !atWork(liveTurn.blocks[liveTurn.blocks.length - 1]);
  function atWork(b: Block | undefined): boolean {
    if (!b) return false;
    if (b.kind === "run") return atWork(b.blocks[b.blocks.length - 1]);
    return b.kind === "text" || b.kind === "thinking" || (b.kind === "group" && b.items.some((i) => i.running));
  }
  // Takes the live turn as an argument so the template re-runs it when the turn ends.
  const isLive = (t: Turn, i: number, now: Turn | null) => t === now && i === t.blocks.length - 1;

  $: newest = (() => {
    const m = new Map<string, number>();
    for (const t of turns) for (const b of t.blocks) if (b.kind === "artifact") m.set(b.id, Math.max(m.get(b.id) ?? 0, b.version ?? 1));
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
  $: lastBlocks = turns.length ? turns[turns.length - 1].blocks : [];
  $: offer = (() => {
    for (let i = lastBlocks.length - 1; i >= 0; i--) {
      const b = lastBlocks[i];
      if (b.kind === "widget" && b.fence === "parzi-replies") return b;
    }
    return null;
  })();
  $: lastError = lastBlocks.length && lastBlocks[lastBlocks.length - 1].kind === "error" ? lastBlocks[lastBlocks.length - 1].key : undefined;
  $: fix = provider ? nextStep($board.find((b) => b.provider === provider)) : null;
  $: fixWait = provider ? $setupWaits.get(provider) : undefined;

  // The reply being written: only the block being written is rendered again
  // each frame; the page patches just the blocks that changed (htmlBlocks).
  $: liveTail = liveTurn?.blocks[liveTurn.blocks.length - 1];
  $: liveSegs = liveTail?.kind === "text" ? splitSegments(liveTail.text, false) : [];
  $: tailIdx = liveSegs.map((s) => s.kind).lastIndexOf("md");
  $: liveBlockHtml = tailIdx >= 0 ? liveHtml(String(liveSegs[tailIdx].body), $richReady) : [];

  /** The last finished test run among a group's steps. */
  function lastTestRun(tools: ToolItem[]): { run: TestRun; command: string } | null {
    for (let i = tools.length - 1; i >= 0; i--) {
      const t = tools[i];
      if (t.running || !t.output) continue;
      const run = testRunOf(t.output);
      if (run) return { run, command: t.cmd ?? "" };
    }
    return null;
  }

  function attachedImages(text: string): string[] {
    const m = /\[attached: ([^\]]+)\]/.exec(text);
    return m ? m[1].split(",").map((s) => s.trim()).filter((s) => IMAGE.test(s)) : [];
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
  {#if firstShown > 0}
    <button class="earlier" on:click={() => (visibleTurns += WINDOW)}>
      Show earlier messages
    </button>
  {/if}
  {#each shownTurns as turn (turn.key)}
    {@const reply = turn === liveTurn ? null : replyOf(turn)}
    <div class="turn">
      {#if turn.user}
        {@const u = turn.user}
        {@const images = attachedImages(u.text)}
        {@const said = splitQuotes(stripMarker(u.text))}
        {@const from = sideFrom(u.text)}
        <div class="row user">
          {#if from}<span class="side-from">Side session from {from}</span>{/if}
          <div class="user-line">
            {#if when(u.at)}<span class="when">{when(u.at)}</span>{/if}
            {#if said.text}
              <button class="copy" use:tooltip={"Copy"} on:click={() => copy(said.text, u.key)}>
                <Icon name={copied === u.key ? "check" : "copy"} />
              </button>
            {/if}
            <button class="copy" use:tooltip={"Edit in composer"} on:click={() => dispatch("edited", { text: said.text })}>
              <Icon name="pencil" />
            </button>
            {#if userNth.has(u.key)}
              <button
                class="copy"
                use:tooltip={"Rewind: go on from before this message"}
                on:click={() => dispatch("rewind", { nth: userNth.get(u.key) ?? 0, text: said.text })}
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
      {/if}
      {#each turn.blocks as b, i (b.key)}
        {#if b.kind === "text"}
          <div class="row">
            <div class="body">
              {#if isLive(turn, i, liveTurn)}
                <!-- Laid out exactly as the saved reply that follows it, so the end of the turn moves nothing. -->
                {#each liveSegs as seg, n}
                  {#if seg.kind === "md" && n === tailIdx}
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
              {:else}
                {#each splitSegments(b.text) as seg}
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
              {/if}
            </div>
            {#if reply === b}
              <div class="actions">
                {#if when(b.at)}<span class="when">{when(b.at)}</span>{/if}
                <button class="copy" use:tooltip={"Copy"} on:click={() => copy(b.text, b.key)}>
                  <Icon name={copied === b.key ? "check" : "copy"} />
                </button>
                <button class="copy" use:tooltip={"Save to projects"} on:click={() => save(b.text, b.key)}>
                  <Icon name={saved === b.key ? "check" : "project"} />
                </button>
                <button class="copy" use:tooltip={"Fork: go on in a copy of this thread"} on:click={() => dispatch("forked")}>
                  <Icon name="fork" />
                </button>
                {#if workedLine(turn)}<span class="when">{workedLine(turn)}</span>{/if}
              </div>
            {/if}
          </div>
        {:else if b.kind === "thinking"}
          <div class="work"><Thinking text={b.text} live={isLive(turn, i, liveTurn)} /></div>
        {:else if b.kind === "group"}
          {@const tests = b.tool === "run" ? lastTestRun(b.items) : null}
          <div class="work">
            <WorkGroup tool={b.tool} items={b.items} live={isLive(turn, i, liveTurn)} />
            {#if tests}<TestCard run={tests.run} command={tests.command} />{/if}
            <ToolPictures {session} paths={picturesIn(b.items.map((t) => t.output ?? ""))} />
          </div>
        {:else if b.kind === "run"}
          <!-- A test result and pictures the work made stay in view when the run folds. -->
          {@const items = b.blocks.flatMap((p) => (p.kind === "group" ? p.items : []))}
          {@const tests = lastTestRun(items.filter((t) => t.kind === "run"))}
          <div class="work">
            <WorkRun blocks={b.blocks} live={isLive(turn, i, liveTurn)} />
            {#if tests}<TestCard run={tests.run} command={tests.command} />{/if}
            <ToolPictures {session} paths={picturesIn(items.map((t) => t.output ?? ""))} />
          </div>
        {:else if b.kind === "echo"}
          <div class="work echo">{b.text}</div>
        {:else if b.kind === "stop"}
          <div class="work stopped">{workedLine(turn) || "Stopped"}</div>
        {:else if b.kind === "error"}
          {@const reset = b.class === "rate_limit" ? resetLine(provider, $board) : ""}
          <div class="turn-error" role="alert" use:tooltip={shortErr(b.message)}>
            <span class="err-dot" aria-hidden="true" />
            <span class="err-class">{ERROR_WORDS[b.class] ?? ERROR_WORDS.unknown}</span>
            <span class="err-msg">{reset ? reset.replace(/\.$/, "") : shortErr(b.message)}</span>
            {#if b.key === lastError && !streaming}
              {#if fixWait}
                <span class="err-msg">{fixWait.note}</span>
              {:else if fix}
                <button class="btn sm" on:click={() => fix?.run(provider)}>{fix.label}</button>
              {:else if b.class === "context_overflow"}
                <button class="btn sm" on:click={() => dispatch("compact")}>Compact</button>
              {:else if b.class === "rate_limit"}
                <button class="btn sm primary" on:click={() => dispatch("pickModel")}>Switch model</button>
                <button class="btn sm" on:click={() => dispatch("retry")}>Try again</button>
              {:else if RETRYABLE.has(b.class)}
                <button class="btn sm" on:click={() => dispatch("retry")}>Try again</button>
              {/if}
            {/if}
          </div>
        {:else if b.kind === "widget"}
          {#if b.fence === "parzi-widget"}
            <Widget data={b.payload} />
          {:else if b.fence === "viz"}
            <Viz spec={b.payload} />
          {:else if b.fence !== "parzi-replies"}
            <div class="msg">{@html mdHtml(fenced(JSON.stringify(b.payload, null, 2), b.fence), $richReady)}</div>
          {/if}
        {:else if b.kind === "artifact"}
          {#if (b.version ?? 1) >= (newest.get(b.id) ?? 0)}
            {#key b.version}
              <ArtifactCard data={b.payload} fallbackId={b.id} fallbackTitle={b.title} fallbackKind={b.artifact_kind} inline />
            {/key}
          {/if}
        {/if}
      {/each}
      {#if turn === liveTurn && waiting}
        <div class="work waiting"><span class="shine">Thinking</span></div>
      {/if}
    </div>
  {/each}

  {#if !streaming && offer}
    <ReplyButtons payload={offer.payload} on:reply={(e) => dispatch("reply", e.detail)} />
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
      <QuestionCard {proposal} on:accept={() => dispatch("proposal", { session })} on:revise={() => dispatch("reviseProposal")} />
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
  /* A turn: the person's message, then what the agent said and did, in
     order. Work in a row (thinking, groups of steps) sits close together. */
  .turn {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
    min-width: 0;
  }
  .turn + .turn {
    margin-top: calc(var(--space) * 4);
  }
  .turn > .work + .work {
    margin-top: calc(var(--space) * -2);
  }
  .work {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    min-width: 0;
  }
  .echo {
    font-family: var(--mono);
    font-size: var(--fs-sm);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .stopped,
  .waiting {
    min-height: var(--h);
    justify-content: center;
    font-size: var(--fs-md);
    color: var(--muted);
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
    color: var(--text);
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
