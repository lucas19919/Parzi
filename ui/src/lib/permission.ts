// Every session starts on full access; the composer's choice travels as
// `mode` and is what runs (launch.rs effective_mode). Only an explicit
// "deny" in the config still locks a session down; every other value
// (ask, auto, full, missing) means full.
export function permissionBlocked(choice: string, defaultMode: string | null | undefined): string {
  return permissionFor(defaultMode) === "deny" && choice !== "deny" ? "Locked in Settings" : "";
}

export function permissionFor(defaultMode: string | null | undefined): string {
  if ((defaultMode ?? "").trim() === "deny") return "deny";
  return "full";
}
