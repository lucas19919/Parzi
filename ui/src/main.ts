import "./theme.css";
import App from "./App.svelte";

try {
  if (localStorage.getItem("parzi.scanlines") === "1") document.documentElement.classList.add("scanlines");
  if (localStorage.getItem("parzi.layout") === "sidebar") document.documentElement.classList.add("parzi-sidebar");
} catch {}

new App({ target: document.getElementById("app")! });
