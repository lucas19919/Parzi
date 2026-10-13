import "./theme.css";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";
import { installCrashHandlers } from "./lib/crash";

// Before the app: an error while it mounts is reported too.
installCrashHandlers((log) => invoke("ui_log", { level: log.level, message: log.message, stack: log.stack }));

try {
  if (localStorage.getItem("parzi.scanlines") === "1") document.documentElement.classList.add("scanlines");
  // The sidebar layout is gone: a saved choice is dropped, the tab bar stays.
  localStorage.removeItem("parzi.layout");
} catch {}

new App({ target: document.getElementById("app")! });
