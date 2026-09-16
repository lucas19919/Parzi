<script lang="ts">
  /**
   * Thin wrapper kept so the shell can mount the project stage by its old
   * name: the stage itself is the deck (PLAN.md §8, ProjectDeck). New code
   * should mount `deck/ProjectDeck.svelte` with a workspace and a slug.
   */
  import { createEventDispatcher } from "svelte";
  import ProjectDeck from "./deck/ProjectDeck.svelte";
  import type { InspectorDoc, LaneView, ProjectRoster, SessionMeta } from "./api";

  export let project = "default";
  /** Workspace of the project; empty = resolve it from the slug. */
  export let workspace = "";

  // The deck reads the project's own files, so the stage no longer needs
  // what the old mission-control page was handed. Kept as accepted props
  // until App.svelte stops passing them (shell lane).
  // svelte-ignore unused-export-let
  export let root = "";
  // svelte-ignore unused-export-let
  export let branch = "";
  // svelte-ignore unused-export-let
  export let lanes: LaneView[] = [];
  // svelte-ignore unused-export-let
  export let threads: SessionMeta[] = [];
  // svelte-ignore unused-export-let
  export let roster: ProjectRoster | null = null;
  // svelte-ignore unused-export-let
  export let plan = "";

  const dispatch = createEventDispatcher<{
    openThread: { id: string };
    newThread: void;
    killRun: { id: string };
    newSubsession: { id: string };
    askHeader: { prompt: string };
    executePlan: void;
    planChanged: { plan: string };
    rosterSaved: { roster: ProjectRoster };
    openDoc: { doc: InspectorDoc };
    openPanel: void;
  }>();
</script>

<ProjectDeck {workspace} slug={project} on:openDoc={(e) => dispatch("openDoc", e.detail)} on:openPanel={() => dispatch("openPanel")} />
