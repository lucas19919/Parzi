import { persisted } from "./browserData";

// The cards under the composer on Home, in the order they show.
export const HOME_WIDGETS = [
  { id: "recent", label: "Recent sessions" },
  { id: "work", label: "Work" },
  { id: "agents", label: "Agents" },
  { id: "projects", label: "Projects" },
  { id: "sites", label: "Pinned sites" },
  { id: "git", label: "Git" },
  { id: "server", label: "Server" },
] as const;

export type HomeWidget = (typeof HOME_WIDGETS)[number]["id"];

// Which cards are hidden: a per-device preference, like the layout.
export const hiddenWidgets = persisted<HomeWidget[]>("parzi.home.hidden.v1", []);

// The Tasks card became the Work card: hiding one hides the other.
hiddenWidgets.update((all) => {
  const list = all as string[];
  return list.includes("tasks") ? ([...new Set(list.map((id) => (id === "tasks" ? "work" : id)))] as HomeWidget[]) : all;
});

export function toggleWidget(id: HomeWidget) {
  hiddenWidgets.update((all) => (all.includes(id) ? all.filter((x) => x !== id) : [...all, id]));
}
