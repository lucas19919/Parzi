import "./theme.css";
import App from "./App.svelte";

try {
  if (localStorage.getItem("parzi.scanlines") === "1") document.documentElement.classList.add("scanlines");
  // The sidebar layout is gone: a saved choice is dropped, the tab bar stays.
  localStorage.removeItem("parzi.layout");
} catch {}

new App({ target: document.getElementById("app")! });
