import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// KaTeX ships woff2 + woff + ttf for every face (~800KB of legacy
// fallbacks). All WebView2/Chromium targets use woff2, so strip the
// woff/ttf urls and they are never emitted.
function katexWoff2Only(): Plugin {
  return {
    name: "katex-woff2-only",
    enforce: "pre",
    transform(code: string, id: string) {
      const path = id.split("?")[0];
      if (path.includes("katex") && path.endsWith(".css")) {
        return code.replace(/,?\s*url\([^)]*?\.(woff|ttf)\)\s*format\("?(woff|truetype)"?\)/g, "");
      }
    },
  };
}

export default defineConfig({
  plugins: [svelte(), katexWoff2Only()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2021",
    rollupOptions: {
      output: {
        // Stable vendor/md chunks keep app-code hashes churn-free
        // across releases; heavy renderers split off by dynamic import.
        manualChunks: {
          vendor: ["svelte", "@tauri-apps/api"],
          md: ["markdown-it", "dompurify"],
        },
      },
    },
  },
});
