<script lang="ts">
  // A goal's page: what was asked, the plan with who worked each step, every
  // run with what it changed and checked, and a follow-up to the lead agent.
  // Read only for now: reviewing (Looks good, Not quite) comes later.
  import { createEventDispatcher, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import StepRail from "./StepRail.svelte";
  import { workApi, type GoalDetail, type GoalView, type RunView, type SessionMeta } from "./api";
  import { mdHtml, richReady } from "./md";
  import { handleLinkClick } from "./links";
  import { prettyModel } from "./providerRows";
  import { tooltip } from "./tooltip";
  import { activity, now, work } from "./workStore";
  import {
    basename,
    cardLine,
    engineLabel,
    formatCost,
    goalActivity,
    runTime,
    runTone,
    runWord,
    statusWord,
    stepProgress,
    stepWord,
    toneOf,
    waitsOn,
    whenOf,
  } from "./workModel";

  export let id: string;
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let projectTitles: Map<string, string> = new Map();
  // Sends a follow-up to the goal's lead session; resolves true once sent.
  export let followUp: (session: string, text: string) => Promise<boolean> = async () => false;

  const dispatch = createEventDispatcher<{ close: void; openSession: { id: string } }>();

  const FILES_SHOWN = 8;

  let detail: GoalDetail | null = null;
  let error = "";
  let loadedKey = "";
  let draft = "";
  let sending = false;
  let openRuns = new Set<string>();
  // Runs whose summary runs past its clamp, so they get Show more.
  let long = new Set<string>();
  let pageEl: HTMLElement | null = null;

  $: summary = $work.goals.find((g) => g.id === id) ?? null;
  // The list is fresher for status, steps and what runs; the detail adds runs and notes.
  $: goal = (summary ?? detail) as GoalView | null;
  $: key = summary ? `${id}|${summary.updated}|${summary.status}|${summary.running.join(",")}` : id;
  $: void load(key);
  $: if (id) resetFor(id);

  let shownId = "";
  async function resetFor(next: string) {
    if (next === shownId) return;
    shownId = next;
    draft = "";
    openRuns = new Set();
    await tick();
    const node = pageEl?.querySelector(".g-scroll");
    if (node) node.scrollTop = 0;
  }

  async function load(k: string) {
    if (k === loadedKey) return;
    loadedKey = k;
    const want = id;
    try {
      const d = await workApi.goal(want);
      if (want !== id) return;
      detail = d;
      error = "";
    } catch (e) {
      if (want !== id) return;
      if (!summary) detail = null;
      error = String(e);
    }
  }

  $: g = goal;
  $: tone = g ? toneOf(g) : "idle";
  $: progress = stepProgress(g?.steps ?? []);
  $: runs = [...(detail?.id === id ? detail.runs : [])].reverse();
  $: notes = detail?.id === id ? detail.notes : [];
  $: live = g && g.status === "working" ? goalActivity(g, $activity) : null;
  $: reason = g && (g.status === "check" || g.status === "failed" || g.status === "paused" || g.status === "needs_you") ? cardLine(g, $activity).text : "";
  $: rootBusy = !!g?.root && (running.has(g.root) || g.running.includes(g.root));
  $: canSend = !!g && g.owned && !!g.root && !rootBusy && !sending;
  $: placeholder = !g?.owned
    ? `Read only here. ${engineLabel(g?.engine ?? { kind: "device", name: "" })} runs this work.`
    : !g?.root
      ? "This work hasn't started yet."
      : rootBusy
        ? "The lead agent is working. Follow up once it stops."
        : "Follow up with the lead agent";
  $: meta = g
    ? [
        g.project ? (projectTitles.get(g.project.toLowerCase()) ?? g.project) : "No project",
        `started ${whenOf(g.created, $now)}`,
        formatCost(g.cost_usd),
        g.files ? `${g.files} file${g.files === 1 ? "" : "s"} changed` : "",
      ].filter(Boolean)
    : [];

  // Reactive, so the template follows new sessions, runs and steps.
  $: titleOf = (session: string): string =>
    threads.find((t) => t.id === session)?.title || `Session ${session.replace(/^(r:|m:[^:]+:)/, "").slice(0, 8)}`;

  $: isLive = (session: string): boolean => running.has(session) || !!g?.running.includes(session);

  $: who = (r: RunView): string => {
    if (r.step) {
      const s = g?.steps.find((x) => x.key === r.step);
      return s ? `Step ${s.key}: ${s.title}` : `Step ${r.step}`;
    }
    return r.session === g?.root ? "Lead agent" : "Helper";
  };

  function dirOf(path: string): string {
    const parts = path.replace(/\\/g, "/").split("/");
    parts.pop();
    return parts.slice(-3).join("/");
  }

  // A step's session titled like the step says nothing new: name it plainly.
  $: sessionLabel = (sid: string, step: string, i: number, n: number): string => {
    const title = titleOf(sid);
    if (title !== step) return title;
    return n > 1 ? `Session ${i + 1}` : "Open session";
  };

  function measure(node: HTMLElement, rid: string) {
    const check = () => {
      if (!node.classList.contains("clamped")) return;
      const over = node.scrollHeight > node.clientHeight + 2;
      if (over === long.has(rid)) return;
      const next = new Set(long);
      if (over) next.add(rid);
      else next.delete(rid);
      long = next;
    };
    const watcher = new ResizeObserver(check);
    watcher.observe(node);
    check();
    return { destroy: () => watcher.disconnect() };
  }

  function toggleRun(rid: string) {
    const next = new Set(openRuns);
    if (next.has(rid)) next.delete(rid);
    else next.add(rid);
    openRuns = next;
  }

  async function send() {
    const text = draft.trim();
    if (!text || !g?.root || !canSend) return;
    sending = true;
    try {
      if (await followUp(g.root, text)) draft = "";
    } finally {
      sending = false;
    }
  }
</script>

<aside class="goal-page" aria-label="Goal" bind:this={pageEl}>
  {#if g}
    <header class="g-head">
      <span class="status {tone}"><i class="sdot" />{statusWord(g)}</span>
      {#if g.engine.kind !== "here"}
        <span class="engine"><Icon name={g.engine.kind === "server" ? "server" : "desktop"} size={12} />{engineLabel(g.engine)}</span>
      {/if}
      <span class="grow" />
      {#if g.root}
        <button class="head-btn" on:click={() => g?.root && dispatch("openSession", { id: g.root })}><Icon name="chat" size={12} />Open session</button>
      {/if}
      <button class="icon-btn" use:tooltip={"Close"} on:click={() => dispatch("close")}><Icon name="close" /></button>
    </header>

    <div class="g-scroll">
      <h2 class="g-title">{g.title}</h2>
      <p class="g-meta">{meta.join(" · ")}</p>

      {#if live}
        <p class="g-live"><i class="pulse" />{live.text}{#if live.more > 0}<span class="also">and {live.more} more agent{live.more === 1 ? "" : "s"}</span>{/if}</p>
      {:else if reason}
        <p class="g-reason {tone}">{reason}</p>
      {/if}
      {#if error && !detail}<p class="g-err">Couldn't load this goal: {error}</p>{/if}

      {#if g.prompt}
        <section class="sec" aria-labelledby="g-asked">
          <h3 id="g-asked">You asked</h3>
          <p class="prompt">{g.prompt}</p>
        </section>
      {/if}

      {#if g.steps.length}
        <section class="sec" aria-labelledby="g-plan">
          <h3 id="g-plan">Plan<span class="n">{progress.done} of {progress.total} done</span></h3>
          <StepRail variant="vertical" steps={g.steps} let:row>
            {@const s = row.step}
            {@const stepLive = s.status === "running" ? goalActivity({ running: s.sessions }, $activity) : null}
            <div class="step">
              <div class="s-line">
                <span class="s-title">{s.title}</span>
                <span class="s-word {s.status}">{stepWord(s)}</span>
              </div>
              {#if s.status === "todo" && s.blocked_by.length}<p class="s-sub">{waitsOn(s, g.steps)}</p>{/if}
              {#if stepLive}<p class="s-live"><i class="pulse" />{stepLive.text}</p>{/if}
              {#if s.outcome}<p class="s-text">{s.outcome}</p>{/if}
              {#if s.note}<p class="s-text note">{s.note}</p>{/if}
              {#if s.sessions.length}
                <div class="s-sessions">
                  {#each s.sessions as sid, i (sid)}
                    <button class="sess" on:click={() => dispatch("openSession", { id: sid })}>
                      <i class="dot" class:live={isLive(sid)} />{sessionLabel(sid, s.title, i, s.sessions.length)}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          </StepRail>
        </section>
      {/if}

      {#if notes.length}
        <section class="sec" aria-labelledby="g-notes">
          <h3 id="g-notes">Notes</h3>
          {#each notes as n, i (i)}
            <p class="note-row"><span class="by">{n.by === "you" ? "You" : n.by}{n.step ? `, step ${n.step}` : ""}</span>{n.text}</p>
          {/each}
        </section>
      {/if}

      <section class="sec" aria-labelledby="g-runs">
        <h3 id="g-runs">Runs{#if runs.length}<span class="n">{runs.length}</span>{/if}</h3>
        {#each runs as r (r.id)}
          {@const rt = runTone(r)}
          {@const more = r.files.length > FILES_SHOWN ? r.files.length - FILES_SHOWN + r.files_more : r.files_more}
          {@const runLive = r.ended === null ? goalActivity({ running: [r.session] }, $activity) : null}
          <article class="run">
            <div class="r-head">
              <i class="rdot {rt}" />
              <span class="r-who">{who(r)}</span>
              <span class="r-meta">{[prettyModel(r.model), runTime(r, $now), formatCost(r.cost_usd)].filter(Boolean).join(" · ")}</span>
              <span class="r-word {rt}">{runWord(r)}</span>
              <button class="icon-btn sm" use:tooltip={`Open ${titleOf(r.session)}`} on:click={() => dispatch("openSession", { id: r.session })}><Icon name="chat" size={12} /></button>
            </div>
            {#if runLive}<p class="s-live"><i class="pulse" />{runLive.text}</p>{/if}
            {#if r.error}<p class="r-err">{r.error}</p>{/if}
            {#if r.summary}
              <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
              <div
                class="r-sum msg"
                class:clamped={!openRuns.has(r.id)}
                class:fade={!openRuns.has(r.id) && long.has(r.id)}
                use:measure={r.id}
                on:click={(e) => void handleLinkClick(e)}
              >
                {@html mdHtml(r.summary, $richReady)}
              </div>
              {#if long.has(r.id)}
                <button class="link" on:click={() => toggleRun(r.id)}>{openRuns.has(r.id) ? "Show less" : "Show more"}</button>
              {/if}
            {/if}
            {#if r.files.length || r.files_more}
              <div class="r-block">
                <span class="label">Files changed</span>
                <ul class="files">
                  {#each r.files.slice(0, FILES_SHOWN) as f (f.path)}
                    <li use:tooltip={f.path}><Icon name="file" size={12} /><span class="fname">{basename(f.path)}</span><span class="fdir">{dirOf(f.path)}</span></li>
                  {/each}
                  {#if more > 0}<li class="fmore">+{more} more</li>{/if}
                </ul>
              </div>
            {/if}
            {#if r.checks.length}
              <div class="r-block">
                <span class="label">Checks</span>
                <ul class="checks">
                  {#each r.checks as c, i (i)}
                    <li class:ok={c.ok}><Icon name={c.ok ? "checkCircle" : "xCircle"} size={12} /><code>{c.cmd}</code><span class="cword">{c.ok ? "Passed" : "Failed"}</span></li>
                  {/each}
                </ul>
              </div>
            {/if}
          </article>
        {:else}
          <p class="empty">{detail || error ? "No runs yet." : "Loading runs…"}</p>
        {/each}
      </section>
    </div>

    <form class="follow" on:submit|preventDefault={send}>
      <input aria-label="Follow up with the lead agent" {placeholder} disabled={!canSend} bind:value={draft} />
      <button class="send" type="submit" disabled={!canSend || !draft.trim()} use:tooltip={"Send to the lead agent"}><Icon name="send" /></button>
    </form>
  {:else}
    <header class="g-head">
      <span class="grow" />
      <button class="icon-btn" use:tooltip={"Close"} on:click={() => dispatch("close")}><Icon name="close" /></button>
    </header>
    <p class="empty">{error ? `Couldn't load this goal: ${error}` : "Loading…"}</p>
  {/if}
</aside>

<style>
  .goal-page {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
  }
  .g-head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 12px 0 24px;
  }
  .grow {
    flex: 1;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 10px 2px 8px;
    border-radius: 999px;
    background: var(--hover);
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
    white-space: nowrap;
  }
  .status.live,
  .status.need,
  .status.bad {
    color: var(--text);
  }
  .status.live {
    background: color-mix(in srgb, var(--ok) 14%, transparent);
  }
  .status.need {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
  }
  .status.bad {
    background: color-mix(in srgb, var(--bad) 18%, transparent);
  }
  .sdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .live .sdot {
    background: var(--ok);
  }
  .need .sdot {
    background: var(--warn);
  }
  .bad .sdot {
    background: var(--bad);
  }
  .ok .sdot {
    background: var(--muted);
  }
  .engine {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--hover);
    color: var(--muted);
    font-size: var(--fs-xs);
    white-space: nowrap;
  }
  .head-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .head-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .icon-btn {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .g-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 24px 24px;
  }
  .g-title {
    margin: 0;
    font-size: var(--fs-xl);
    font-weight: 600;
    line-height: 1.3;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  .g-meta {
    margin: 6px 0 0;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .g-live,
  .g-reason {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 14px 0 0;
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--hover);
    font-size: var(--fs-md);
  }
  .g-reason.need {
    background: color-mix(in srgb, var(--warn) 12%, transparent);
  }
  .g-reason.bad {
    background: color-mix(in srgb, var(--bad) 14%, transparent);
  }
  .also {
    color: var(--muted);
  }
  .g-err {
    margin: 14px 0 0;
    font-size: var(--fs-md);
    color: var(--bad);
  }
  .sec {
    margin-top: 24px;
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 10px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .n {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .prompt {
    margin: 0;
    padding: 10px 14px;
    border-left: 2px solid var(--line);
    background: color-mix(in srgb, var(--text) 3%, transparent);
    border-radius: 0 var(--radius) var(--radius) 0;
    font-size: var(--fs-md);
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .step {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 5px 0 16px;
  }
  .s-line {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .s-title {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-md);
    font-weight: 500;
    line-height: 20px;
    overflow-wrap: anywhere;
  }
  .s-word {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .s-word.running {
    color: var(--ok);
    font-weight: 600;
  }
  .s-word.failed,
  .s-word.check {
    color: var(--text);
    font-weight: 600;
  }
  .s-sub,
  .s-text,
  .s-live {
    margin: 0;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .s-live {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }
  .s-text.note {
    font-style: italic;
  }
  .s-sessions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 4px;
  }
  .sess {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    min-height: 24px;
    padding: 2px 10px 2px 8px;
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .sess:hover {
    background: var(--hover);
    color: var(--text);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.live {
    background: var(--ok);
  }
  .pulse {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    animation: pulse 1.6s var(--ease-out) infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .note-row {
    margin: 0 0 8px;
    font-size: var(--fs-md);
    line-height: 1.5;
  }
  .by {
    margin-right: 8px;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  .run {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 10px;
    padding: 10px 14px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--text) 2%, transparent);
  }
  .r-head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .rdot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint);
  }
  .rdot.live {
    background: var(--ok);
  }
  .rdot.ok {
    background: var(--muted);
  }
  .rdot.need {
    background: var(--warn);
  }
  .rdot.bad {
    background: var(--bad);
  }
  .r-who {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    font-weight: 600;
  }
  .r-meta {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .r-word {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .r-word.live,
  .r-word.need,
  .r-word.bad {
    color: var(--text);
    font-weight: 600;
  }
  .r-err {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--bad);
  }
  .r-sum {
    font-size: var(--fs-md);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .r-sum.clamped {
    max-height: calc(1.55em * 4);
    overflow: hidden;
  }
  .r-sum.fade {
    mask-image: linear-gradient(to bottom, #000 60%, transparent);
  }
  .r-sum :global(p) {
    margin: 0 0 0.4em;
  }
  .link {
    align-self: flex-start;
    min-height: 24px;
    padding: 0 6px;
    margin-left: -6px;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .link:hover {
    background: var(--hover);
    color: var(--text);
  }
  .r-block {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .label {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  .files,
  .checks {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .files li,
  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 22px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .fname {
    flex: none;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono), ui-monospace, monospace;
    color: var(--text);
  }
  .fdir {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-xs);
  }
  .fmore {
    padding-left: 20px;
  }
  .checks li {
    color: var(--bad);
  }
  .checks li.ok {
    color: var(--ok);
  }
  .checks code {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .cword {
    flex: none;
    margin-left: auto;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .icon-btn.sm {
    flex: none;
    width: 24px;
    height: 24px;
    margin: -2px -6px -2px 0;
  }
  .empty {
    margin: 0;
    font-size: var(--fs-md);
    color: var(--muted);
  }
  .goal-page > .empty {
    padding: 8px 24px;
  }
  .follow {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 16px 16px;
    padding: 4px 4px 4px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--glass-strong-bg);
  }
  .follow input {
    flex: 1;
    min-width: 0;
    height: 32px;
    background: none;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .follow input:disabled {
    cursor: default;
  }
  .send {
    flex: none;
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--accent);
    border: none;
    border-radius: var(--radius);
    color: var(--bg);
    cursor: pointer;
  }
  .send:disabled {
    background: var(--hover);
    color: var(--muted);
    cursor: default;
  }
</style>
