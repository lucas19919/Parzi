import { api, type ParziConfig } from "./api";

export async function updateConfig(change: (c: ParziConfig) => void): Promise<ParziConfig> {
  const fresh = await api.getConfig();
  change(fresh);
  await api.saveConfig(fresh);
  return fresh;
}
