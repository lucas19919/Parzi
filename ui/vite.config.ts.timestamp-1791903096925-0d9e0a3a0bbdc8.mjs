// vite.config.ts
import { defineConfig } from "file:///D:/Repos/Parzi/ui/node_modules/vite/dist/node/index.js";
import { svelte } from "file:///D:/Repos/Parzi/ui/node_modules/@sveltejs/vite-plugin-svelte/src/index.js";
function katexWoff2Only() {
  return {
    name: "katex-woff2-only",
    enforce: "pre",
    transform(code, id) {
      const path = id.split("?")[0];
      if (path.includes("katex") && path.endsWith(".css")) {
        return code.replace(/,?\s*url\([^)]*?\.(woff|ttf)\)\s*format\("?(woff|truetype)"?\)/g, "");
      }
    }
  };
}
var vite_config_default = defineConfig({
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
          md: ["markdown-it", "dompurify"]
        }
      }
    }
  }
});
export {
  vite_config_default as default
};
//# sourceMappingURL=data:application/json;base64,ewogICJ2ZXJzaW9uIjogMywKICAic291cmNlcyI6IFsidml0ZS5jb25maWcudHMiXSwKICAic291cmNlc0NvbnRlbnQiOiBbImNvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9kaXJuYW1lID0gXCJEOlxcXFxSZXBvc1xcXFxQYXJ6aVxcXFx1aVwiO2NvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9maWxlbmFtZSA9IFwiRDpcXFxcUmVwb3NcXFxcUGFyemlcXFxcdWlcXFxcdml0ZS5jb25maWcudHNcIjtjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfaW1wb3J0X21ldGFfdXJsID0gXCJmaWxlOi8vL0Q6L1JlcG9zL1BhcnppL3VpL3ZpdGUuY29uZmlnLnRzXCI7aW1wb3J0IHsgZGVmaW5lQ29uZmlnLCB0eXBlIFBsdWdpbiB9IGZyb20gXCJ2aXRlXCI7XG5pbXBvcnQgeyBzdmVsdGUgfSBmcm9tIFwiQHN2ZWx0ZWpzL3ZpdGUtcGx1Z2luLXN2ZWx0ZVwiO1xuXG4vLyBLYVRlWCBzaGlwcyB3b2ZmMiArIHdvZmYgKyB0dGYgZm9yIGV2ZXJ5IGZhY2UgKH44MDBLQiBvZiBsZWdhY3lcbi8vIGZhbGxiYWNrcykuIEFsbCBXZWJWaWV3Mi9DaHJvbWl1bSB0YXJnZXRzIHVzZSB3b2ZmMiwgc28gc3RyaXAgdGhlXG4vLyB3b2ZmL3R0ZiB1cmxzIGFuZCB0aGV5IGFyZSBuZXZlciBlbWl0dGVkLlxuZnVuY3Rpb24ga2F0ZXhXb2ZmMk9ubHkoKTogUGx1Z2luIHtcbiAgcmV0dXJuIHtcbiAgICBuYW1lOiBcImthdGV4LXdvZmYyLW9ubHlcIixcbiAgICBlbmZvcmNlOiBcInByZVwiLFxuICAgIHRyYW5zZm9ybShjb2RlOiBzdHJpbmcsIGlkOiBzdHJpbmcpIHtcbiAgICAgIGNvbnN0IHBhdGggPSBpZC5zcGxpdChcIj9cIilbMF07XG4gICAgICBpZiAocGF0aC5pbmNsdWRlcyhcImthdGV4XCIpICYmIHBhdGguZW5kc1dpdGgoXCIuY3NzXCIpKSB7XG4gICAgICAgIHJldHVybiBjb2RlLnJlcGxhY2UoLyw/XFxzKnVybFxcKFteKV0qP1xcLih3b2ZmfHR0ZilcXClcXHMqZm9ybWF0XFwoXCI/KHdvZmZ8dHJ1ZXR5cGUpXCI/XFwpL2csIFwiXCIpO1xuICAgICAgfVxuICAgIH0sXG4gIH07XG59XG5cbmV4cG9ydCBkZWZhdWx0IGRlZmluZUNvbmZpZyh7XG4gIHBsdWdpbnM6IFtzdmVsdGUoKSwga2F0ZXhXb2ZmMk9ubHkoKV0sXG4gIGNsZWFyU2NyZWVuOiBmYWxzZSxcbiAgc2VydmVyOiB7IHBvcnQ6IDE0MjAsIHN0cmljdFBvcnQ6IHRydWUgfSxcbiAgYnVpbGQ6IHtcbiAgICB0YXJnZXQ6IFwiZXMyMDIxXCIsXG4gICAgcm9sbHVwT3B0aW9uczoge1xuICAgICAgb3V0cHV0OiB7XG4gICAgICAgIC8vIFN0YWJsZSB2ZW5kb3IvbWQgY2h1bmtzIGtlZXAgYXBwLWNvZGUgaGFzaGVzIGNodXJuLWZyZWVcbiAgICAgICAgLy8gYWNyb3NzIHJlbGVhc2VzOyBoZWF2eSByZW5kZXJlcnMgc3BsaXQgb2ZmIGJ5IGR5bmFtaWMgaW1wb3J0LlxuICAgICAgICBtYW51YWxDaHVua3M6IHtcbiAgICAgICAgICB2ZW5kb3I6IFtcInN2ZWx0ZVwiLCBcIkB0YXVyaS1hcHBzL2FwaVwiXSxcbiAgICAgICAgICBtZDogW1wibWFya2Rvd24taXRcIiwgXCJkb21wdXJpZnlcIl0sXG4gICAgICAgIH0sXG4gICAgICB9LFxuICAgIH0sXG4gIH0sXG59KTtcbiJdLAogICJtYXBwaW5ncyI6ICI7QUFBNk8sU0FBUyxvQkFBaUM7QUFDdlIsU0FBUyxjQUFjO0FBS3ZCLFNBQVMsaUJBQXlCO0FBQ2hDLFNBQU87QUFBQSxJQUNMLE1BQU07QUFBQSxJQUNOLFNBQVM7QUFBQSxJQUNULFVBQVUsTUFBYyxJQUFZO0FBQ2xDLFlBQU0sT0FBTyxHQUFHLE1BQU0sR0FBRyxFQUFFLENBQUM7QUFDNUIsVUFBSSxLQUFLLFNBQVMsT0FBTyxLQUFLLEtBQUssU0FBUyxNQUFNLEdBQUc7QUFDbkQsZUFBTyxLQUFLLFFBQVEsbUVBQW1FLEVBQUU7QUFBQSxNQUMzRjtBQUFBLElBQ0Y7QUFBQSxFQUNGO0FBQ0Y7QUFFQSxJQUFPLHNCQUFRLGFBQWE7QUFBQSxFQUMxQixTQUFTLENBQUMsT0FBTyxHQUFHLGVBQWUsQ0FBQztBQUFBLEVBQ3BDLGFBQWE7QUFBQSxFQUNiLFFBQVEsRUFBRSxNQUFNLE1BQU0sWUFBWSxLQUFLO0FBQUEsRUFDdkMsT0FBTztBQUFBLElBQ0wsUUFBUTtBQUFBLElBQ1IsZUFBZTtBQUFBLE1BQ2IsUUFBUTtBQUFBO0FBQUE7QUFBQSxRQUdOLGNBQWM7QUFBQSxVQUNaLFFBQVEsQ0FBQyxVQUFVLGlCQUFpQjtBQUFBLFVBQ3BDLElBQUksQ0FBQyxlQUFlLFdBQVc7QUFBQSxRQUNqQztBQUFBLE1BQ0Y7QUFBQSxJQUNGO0FBQUEsRUFDRjtBQUNGLENBQUM7IiwKICAibmFtZXMiOiBbXQp9Cg==
