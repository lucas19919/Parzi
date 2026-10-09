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
