// Entry for mock.html: install the mock backend, then boot the real app.
const { mockView, mockLive } = await import("./mockTauri");
await import("../main");

if (mockView === "settings") {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: ",", ctrlKey: true }));
}
if (mockView === "page") {
  // The app subscribes on mount; then a page asks for the camera.
  const { emit } = await import("@tauri-apps/api/event");
  await new Promise((r) => setTimeout(r, 300));
  await emit("parzi://browser-ask", { kind: "permission", id: "p1", tab: "t-page", origin: "https://meet.google.com", permission: "camera" });
}
if (mockLive) {
  // What the running goals' agents are doing, as their tool calls stream in.
  const { emit } = await import("@tauri-apps/api/event");
  const LIVE: [string, string[]][] = [
    ["s6", ["Editing D:\\Repos\\Parzi\\ui\\src\\lib\\Omnibar.svelte", "Running `npm test -- autosize`", "Reading ui/src/lib/Omnibar.svelte"]],
    ["s3", ["Editing crates/parzi-core/src/sync.rs", "Running `cargo test -p parzi-core sync`"]],
    ["r:a2", ["Running `cargo update -p serde`", "Running `cargo test`"]],
  ];
  let beat = 0;
  const tick = () => {
    for (const [session, labels] of LIVE) void emit("parzi://run-event", { kind: "tool_call", session, id: `live-${beat}`, name: "Edit", label: labels[beat % labels.length] });
    beat++;
  };
  await new Promise((r) => setTimeout(r, 300));
  tick();
  setInterval(tick, 4000);
}
document.documentElement.setAttribute("data-app-ready", "");

export {};
