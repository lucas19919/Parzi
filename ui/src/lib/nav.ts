/** Which threads the sidebar lists as chats. */

/**
 * A chat is any thread that is not a project role session. Chats belong to
 * a workspace (or none); the header, orchestrator and coder sessions of a
 * project carry that project's slug and live in the project's panel.
 */
export function isChatThread(
  project: string | undefined,
  projectSlugs: readonly string[],
): boolean {
  const p = (project || "default").trim() || "default";
  if (p === "default") return true;
  return !projectSlugs.includes(p);
}
