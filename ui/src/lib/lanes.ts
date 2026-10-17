// Single place for legacy lane names ("code"/"research") and lane icons.
export function normLane(lane: string): string {
  if (lane === "code") return "build";
  if (lane === "research") return "work";
  return lane;
}

export function laneIcon(lane: string): "brain" | "bot" | "chat" {
  const l = normLane(lane);
  return l === "work" ? "brain" : l === "build" ? "bot" : "chat";
}

// Settings' permission (lanes.default_mode: ask / auto / deny) is where
// the composer starts; the composer's choice travels as `mode` and is what
// runs (launch.rs effective_mode). Only Locked holds over it. Unknown or
// missing values ask; nothing here ever yields "full".
export function permissionBlocked(choice: string, defaultMode: string | null | undefined): string {
  return permissionFor(defaultMode) === "deny" && choice !== "deny" ? "Locked in Settings" : "";
}

export function permissionFor(defaultMode: string | null | undefined): string {
  const m = (defaultMode ?? "").trim();
  if (m === "auto") return "auto";
  if (m === "deny") return "deny";
  return "supervised";
}
