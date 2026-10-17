import { api, type ParziConfig } from "./api";

// Every settings change goes through here: read the newest config, change
// only what was asked, save. A section never writes its old copy back over
// settings that arrived by sync while it was open. Resolves to the saved
// config; throws when saving failed or the person declined the change.
export async function updateConfig(change: (c: ParziConfig) => void): Promise<ParziConfig> {
  const fresh = await api.getConfig();
  change(fresh);
  await api.saveConfig(fresh);
  return fresh;
}
