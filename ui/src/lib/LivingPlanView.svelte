<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { api } from "./api";

  export let project = "default";
  export let plan = "";

  const dispatch = createEventDispatcher<{ execute: void; changed: { plan: string } }>();

  let saving = false;
  let err = "";
  let newTask = "";
  let newLane = "";

  interface Row { done: boolean; title: string; lane: string | null; worktree: boolean; raw: string }
  interface Group { title: string; rows: Row[] }

  function parse(text: string): Group[] {
    const groups: Group[] = [];
    let cur: Group = { title: "Plan", rows: [] };
    for (const line of text.split("\n")) {
      const t = line.trim();
      if (t.startsWith("## ")) {
        if (cur.rows.length || cur.title !== "Plan") groups.push(cur);
        cur = { title: t.replace(/^#+\s*/, ""), rows: [] };
        continue;
      }
      const m = t.match(/^-\s*\[( |x|X|\/|-)\]\s*(.*)$/);
      if (m) {
        const done = m[1].toLowerCase() === "x";
        let body = m[2];
        let lane: string | null = null;
        const lm = body.match(/\[lane:([^\]]+)\]/);
        if (lm) {
          lane = lm[1].trim();
          body = body.replace(lm[0], "").trim();
        }
        let worktree = false;
        if (body.includes("[worktree]") || body.includes("[isolated]")) {
          worktree = true;
          body = body.replace(/\[worktree\]|\[isolated\]/g, "").trim();
        }
        cur.rows.push({ done, title: body, lane, worktree, raw: line });
      }
    }
    groups.push(cur);
    return groups;
  }

  $: groups = parse(plan);

  async function toggle(row: Row) {
    saving = true;
    err = "";
    try {
      const lines = plan.split("\n");
      const idx = lines.findIndex((l) => l === row.raw);
      if (idx >= 0) {
        const body = lines[idx].trim().slice(5).trim();
        lines[idx] = `- [${row.done ? " " : "x"}] ${body}`;
        const next = lines.join("\n");
        await api.saveProjectPlan(project, next);
        plan = next;
        dispatch("changed", { plan: next });
      }
    } catch (e) {
      err = String(e);
    } finally {
      saving = false;
    }
  }

  async function add() {
    if (!newTask.trim()) return;
    saving = true;
    err = "";
    try {
      const tag = newLane.trim() ? ` [lane:${newLane.trim()}]` : "";
      const next = (plan.endsWith("\n") ? plan : plan + "\n") + `- [ ] ${newTask.trim()}${tag}\n`;
      await api.saveProjectPlan(project, next);
      plan = next;
      newTask = "";
      dispatch("changed", { plan: next });
    } catch (e) {
      err = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="plan">
  <div class="plan-head">
    <span class="plan-title">Living plan</span>
    <span class="spacer" />
    <button class="exec-btn" on:click={() => dispatch("execute")}>Execute plan</button>
  </div>
  {#each groups as g}
    <div class="group">{g.title}</div>
    {#each g.rows as r}
      <label class="row">
        <input type="checkbox" checked={r.done} disabled={saving} on:change={() => toggle(r)} />
        <span class="t" class:done={r.done}>{r.title}</span>
        {#if r.lane}<span class="lane">{r.lane}</span>{/if}
        {#if r.worktree}<span class="wt-badge">worktree</span>{/if}
      </label>
    {/each}
  {/each}
  <div class="add">
    <input class="in" placeholder="new task…" bind:value={newTask} on:keydown={(e) => e.key === "Enter" && add()} />
    <input class="in lane-in" placeholder="lane" bind:value={newLane} on:keydown={(e) => e.key === "Enter" && add()} />
    <button class="exec-btn" on:click={add} disabled={saving}>Add</button>
  </div>
  {#if err}<div class="err">{err}</div>{/if}
</div>

<style>
  .plan {
    background: var(--surface-1); border: 1px solid var(--line);
    border-radius: 12px; padding: 14px 16px; display: flex; flex-direction: column; gap: 6px;
  }
  .plan-head { display: flex; align-items: center; gap: 8px; }
  .plan-title { font-size: 13px; font-weight: 700; color: var(--text); }
  .spacer { flex: 1; }
  .exec-btn {
    background: var(--accent-soft); border: 1px solid var(--accent-line); color: var(--accent-text);
    font: inherit; font-size: 12px; padding: 6px 12px; border-radius: 7px; cursor: pointer; white-space: nowrap;
  }
  .group { font-size: 11px; font-weight: 700; letter-spacing: 0.5px; text-transform: uppercase; color: var(--text-3); margin-top: 8px; }
  .row { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text); cursor: pointer; }
  .row .t.done { text-decoration: line-through; opacity: 0.6; }
  .lane { font-size: 10px; color: var(--accent); background: var(--accent-soft); border-radius: 4px; padding: 1px 6px; }
  .wt-badge { font-size: 9.5px; font-family: var(--parzi-mono), monospace; color: var(--ok); background: var(--ok-soft); border: 1px solid var(--ok-line); border-radius: 4px; padding: 1px 5px; }
  .add { display: flex; gap: 6px; margin-top: 8px; }
  .in { flex: 1; background: var(--surface-2); border: 1px solid var(--line); color: var(--text); border-radius: 7px; padding: 7px 10px; font: inherit; font-size: 12.5px; }
  .lane-in { max-width: 110px; }
  .err { color: var(--bad); font-size: 11.5px; }
</style>
