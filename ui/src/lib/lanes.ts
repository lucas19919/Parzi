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

// Settings' tool policy (lanes.default_mode: ask / auto / deny) to the
// composer's permission, which travels as `mode` on send. Unknown or
// missing values ask; nothing here ever yields "full".
// Settings' default is a floor the composer can tighten but not lift
// (launch.rs restrict_mode). Full access lifts it, except Lockdown.
// Returns why a choice would be ignored, or "" when it applies.
export function permissionBlocked(choice: string, defaultMode: string | null | undefined): string {
  const floor = permissionFor(defaultMode);
  if (floor === "deny") return choice === "deny" ? "" : "Lockdown is on in Settings › General";
  if (floor === "supervised" && choice === "auto") return "Needs Auto in Settings › General";
  return "";
}

export function permissionFor(defaultMode: string | null | undefined): string {
  const m = (defaultMode ?? "").trim();
  if (m === "auto") return "auto";
  if (m === "deny") return "deny";
  return "supervised";
}
