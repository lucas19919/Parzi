<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import { api, onRunEvent, type HostJob } from "./api";
  import type { ArtifactDoc } from "./tabs";
  import { toastError } from "./toast";
  import { tooltip } from "./tooltip";

  /** Localhost servers discovered in this session, with one-click start/restart/stop. */
  export let session = "";

  const dispatch = createEventDispatcher<{ openUrl: { url: string }; openFile: ArtifactDoc }>();

  let hosts: HostJob[] = [];
  let loadedFor = "";
  let busy = new Set<string>();

  $: if (session !== loadedFor) {
    loadedFor = session;
    hosts = [];
    busy = new Set();
    void load();
  }

  async function load() {
    const sid = session;
    const got = sid ? await api.shellHosts(sid).catch(() => [] as HostJob[]) : [];
    if (sid === session) hosts = got;
  }

  $: liveCount = hosts.filter((h) => h.running).length;

  function hostLabel(url: string): string {
    return url.replace(/^https?:\/\//i, "").replace(/\/$/, "");
  }

  function crashed(h: HostJob): boolean {
    return !h.running && h.exit !== null && h.exit !== 0 && h.exit !== -1;
  }

  function hostTip(h: HostJob, url: string): string {
    const parts = [`Open ${url} in Parzi`, h.cmd];
    if (h.workdir) parts.push(h.workdir);
    return parts.join("\n");
  }

  function statusLine(h: HostJob): string {
    if (h.running) return h.cmd;
    if (crashed(h)) return `exited ${h.exit} · ${h.cmd}`;
    return `stopped · ${h.cmd}`;
  }

  function setBusy(id: string, on: boolean) {
    const next = new Set(busy);
    if (on) next.add(id);
    else next.delete(id);
    busy = next;
  }

  async function restart(h: HostJob) {
    if (!session || busy.has(h.id)) return;
    setBusy(h.id, true);
    try {
      await api.shellRestartHost(session, h.id);
      await load();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(h.id, false);
    }
  }

  async function stop(h: HostJob) {
    if (!session || !h.job_id || busy.has(h.id)) return;
    setBusy(h.id, true);
    try {
      await api.shellKill(session, h.job_id);
      await load();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(h.id, false);
    }
  }

  async function remove(h: HostJob) {
    if (!session || busy.has(h.id)) return;
    setBusy(h.id, true);
    try {
      await api.shellRemoveHost(session, h.id);
      await load();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(h.id, false);
    }
  }

  async function openLogs(h: HostJob) {
    if (!session || !h.job_id) return;
    try {
      const tail = await api.shellLogs(session, h.job_id);
      const label = h.urls[0] ? hostLabel(h.urls[0]) : h.cmd;
      dispatch("openFile", {
        id: `host-log:${h.id}`,
        title: `${label}.log`,
        kind: "text",
        content: tail?.text?.trim() ? tail.text : "(no output yet)",
        version: Date.now(),
      });
    } catch (e) {
      toastError(e);
    }
  }

  onMount(() => {
    const timer = setInterval(() => {
      if (liveCount > 0 || busy.size > 0) void load();
    }, 2000);
    const onFocus = () => {
      if (session) void load();
    };
    window.addEventListener("focus", onFocus);
    const un = onRunEvent((e) => {
      if (e.session !== session) return;
      if (e.kind === "tool_call" || e.kind === "tool_result" || e.kind === "done" || e.kind === "error") {
        void load();
      }
    });
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

{#if hosts.length}
  <InfoCard id="hosts" title="Hosts">
    <svelte:fragment slot="aside">{liveCount ? `${liveCount} live` : "stopped"}</svelte:fragment>
    {#each hosts as h (h.id)}
      {@const primary = h.urls[0] ?? ""}
      {@const pending = busy.has(h.id)}
      <div class="info-line host" class:stopped={!h.running}>
        <button
          class="host-open"
          use:tooltip={primary ? hostTip(h, primary) : h.cmd}
          on:click={() => primary && dispatch("openUrl", { url: primary })}
        >
          <i class="info-dot" class:t-ok={h.running} class:t-bad={crashed(h)} />
          <span class="host-meta">
            <span class="info-v mono">{primary ? hostLabel(primary) : h.cmd}</span>
            <span class="cmd mono" class:bad={crashed(h)}>{statusLine(h)}</span>
          </span>
        </button>
        <div class="acts">
          {#if h.running && primary}
            <button
              class="act"
              aria-label="Open {hostLabel(primary)} in browser"
              use:tooltip={"Open in system browser"}
              on:click={() => api.openConfirmedUrl(primary).catch(() => {})}
            >
              <Icon name="external" size={12} />
            </button>
          {/if}
          {#if h.job_id}
            <button
              class="act"
              aria-label="View logs for {primary ? hostLabel(primary) : h.cmd}"
              use:tooltip={"View logs"}
              on:click={() => openLogs(h)}
            >
              <Icon name="terminal" size={12} />
            </button>
          {/if}
          <button
            class="act"
            class:spin={pending}
            disabled={pending}
            aria-label="{h.running ? 'Restart' : 'Start'} {primary ? hostLabel(primary) : h.cmd}"
            use:tooltip={h.running ? "Restart" : "Start"}
            on:click={() => restart(h)}
          >
            <Icon name={h.running || pending ? "reload" : "play"} size={12} />
          </button>
          {#if h.running && h.job_id}
            <button
              class="act"
              disabled={pending}
              aria-label="Stop {primary ? hostLabel(primary) : h.cmd}"
              use:tooltip={"Stop"}
              on:click={() => stop(h)}
            >
              <span class="square" />
            </button>
          {:else}
            <button
              class="act"
              disabled={pending}
              aria-label="Remove {primary ? hostLabel(primary) : h.cmd}"
              use:tooltip={"Remove"}
              on:click={() => remove(h)}
            >
              <Icon name="close" size={12} />
            </button>
          {/if}
        </div>
      </div>
      {#if h.urls.length > 1}
        {#each h.urls.slice(1) as url (url)}
          <div class="info-line extra">
            <button class="host-open" use:tooltip={hostTip(h, url)} on:click={() => dispatch("openUrl", { url })}>
              <span class="info-v mono">{hostLabel(url)}</span>
            </button>
            {#if h.running}
              <button
                class="act"
                aria-label="Open {hostLabel(url)} in browser"
                use:tooltip={"Open in system browser"}
                on:click={() => api.openConfirmedUrl(url).catch(() => {})}
              >
                <Icon name="external" size={12} />
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    {/each}
  </InfoCard>
{/if}

<style>
  .host {
    align-items: center;
    gap: var(--space);
  }
  .host.stopped .info-v {
    color: var(--muted);
  }
  .extra {
    padding-left: calc(var(--space) * 4);
    gap: var(--space);
  }
  .host-open {
    flex: 1;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 1.5);
    padding: calc(var(--space) / 2) var(--space);
    margin: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .host-open:hover {
    background: var(--hover);
  }
  .host-open .info-dot {
    margin-right: 0;
  }
  .host-meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .cmd {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .cmd.bad {
    color: var(--bad-text);
  }
  .acts {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) / 2);
  }
  .act {
    width: 20px;
    height: 20px;
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
  }
  .act:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .act:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .act:hover:not(:disabled) :global(svg) {
    color: var(--text);
  }
  .act.spin :global(svg) {
    animation: host-spin 900ms linear infinite;
  }
  .square {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-sm);
    background: var(--muted);
  }
  .act:hover:not(:disabled) .square {
    background: var(--text);
  }
  .info-dot.t-ok {
    background: var(--ok);
  }
  .info-dot.t-bad {
    background: var(--bad);
  }
  @keyframes host-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
