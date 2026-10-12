import { persisted } from "./browserData";

// The cards under the composer on Home, in the order they show.
export const HOME_WIDGETS = [
  { id: "recent", label: "Recent sessions" },
  { id: "agents", label: "Agents" },
  { id: "projects", label: "Projects" },
  { id: "sites", label: "Pinned sites" },
  { id: "git", label: "Git" },
  { id: "server", label: "Server" },
] as const;

export type HomeWidget = (typeof HOME_WIDGETS)[number]["id"];

// Which cards are hidden: a per-device preference, like the layout.
export const hiddenWidgets = persisted<HomeWidget[]>("parzi.home.hidden.v1", []);

export function toggleWidget(id: HomeWidget) {
  hiddenWidgets.update((all) => (all.includes(id) ? all.filter((x) => x !== id) : [...all, id]));
}
