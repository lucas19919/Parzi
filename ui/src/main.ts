import "./theme.css";
import App from "./App.svelte";

try {
  if (localStorage.getItem("parzi.scanlines") === "1") document.documentElement.classList.add("scanlines");
} catch {}

const app = new App({ target: document.getElementById("app")! });
export default app;
