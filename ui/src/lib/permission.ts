export function permissionBlocked(choice: string, defaultMode: string | null | undefined): string {
  return permissionFor(defaultMode) === "deny" && choice !== "deny" ? "Locked in Settings" : "";
}

export function permissionFor(defaultMode: string | null | undefined): string {
  if ((defaultMode ?? "").trim() === "deny") return "deny";
  return "full";
}
