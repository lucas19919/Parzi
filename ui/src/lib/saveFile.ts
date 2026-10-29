import { invoke } from "@tauri-apps/api/core";
import { keyedToast, toast } from "./toast";

export async function saveFile(name: string, content: string): Promise<void> {
  const data = /^data:[^,]*;base64,/.exec(content);
  let body = content;
  let base64 = false;
  if (data) {
    body = content.slice(data[0].length);
    base64 = true;
  } else if (content.startsWith("data:")) {
    body = decodeURIComponent(content.slice(content.indexOf(",") + 1));
  }
  try {
    const path = await invoke<string | null>("save_as", { name, content: body, base64 });
    if (!path) return;
    const shown = path.split(/[\\/]/).pop() || path;
    keyedToast(`saved-${path}`, `Saved ${shown}`, {
      ms: 5000,
      action: { label: "Show in folder", run: () => void invoke("reveal_saved", { path }).catch((e) => toast(String(e), true)) },
    });
  } catch (e) {
    toast(`Couldn't save: ${e}`, true);
  }
}
