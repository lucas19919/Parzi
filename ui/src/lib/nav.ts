/** Sidebar place: chats are general work, workspaces are projects. */

export type NavMode = "chats" | "workspaces";

export interface Nav {
  mode: NavMode;
  /** Hub workspace name while drilled in; empty on the workspace list. */
  workspace: string;
}

const KEY = "parzi.nav";

export function loadNav(): Nav {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { mode: "chats", workspace: "" };
    const v = JSON.parse(raw) as Partial<Nav>;
    return {
      mode: v.mode === "workspaces" ? "workspaces" : "chats",
      workspace: typeof v.workspace === "string" ? v.workspace : "",
    };
  } catch {
    return { mode: "chats", workspace: "" };
  }
}

export function saveNav(nav: Nav): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(nav));
  } catch {
    /* private mode, quota — the next launch starts on Chats */
  }
}

/**
 * A chat is Inbox (`default`) or a leftover named folder that is not a hub
 * workspace. Header/coder sessions live on the workspace, not here.
 */
export function isChatThread(
  project: string | undefined,
  hubNames: readonly string[],
): boolean {
  const p = (project || "default").trim() || "default";
  if (p === "default") return true;
  return !hubNames.includes(p);
}
