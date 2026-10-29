import "./theme.css";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";

function report(what: unknown) {
  const text = what instanceof Error ? `${what.message}\n${what.stack ?? ""}` : String(what);
  void invoke("ui_error", { message: text }).catch(() => {});
}
window.addEventListener("error", (e) => report(e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) => report(e.reason));

new App({ target: document.getElementById("app")! });
