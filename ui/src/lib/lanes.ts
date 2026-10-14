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
// (launch.rs restrict_mode), except Full access, which lifts any floor but
// Lockdown. A choice is either open, off (disabled with a reason), or a
// lift (works, but says so: it behaves above the floor for this send).
export interface PermGate {
  off: string;
  lift: string;
}

export function permissionGate(choice: string, defaultMode: string | null | undefined): PermGate {
  const floor = permissionFor(defaultMode);
  if (floor === "deny") {
    return choice === "deny"
      ? { off: "", lift: "" }
      : { off: "Lockdown is on in Settings › General", lift: "" };
  }
  if (floor === "supervised" && choice === "auto") {
    return { off: "Ask applies until Auto is on in Settings › General", lift: "" };
  }
  if (floor === "supervised" && choice === "full") {
    return { off: "", lift: "Lifts to Auto for this send" };
  }
  return { off: "", lift: "" };
}

export function permissionBlocked(choice: string, defaultMode: string | null | undefined): string {
  return permissionGate(choice, defaultMode).off;
}

export function permissionFor(defaultMode: string | null | undefined): string {
  const m = (defaultMode ?? "").trim();
  if (m === "auto") return "auto";
  if (m === "deny") return "deny";
  return "supervised";
}
