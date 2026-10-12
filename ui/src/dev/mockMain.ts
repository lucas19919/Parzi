// Entry for mock.html: install the mock backend, then boot the real app.
const { mockView } = await import("./mockTauri");
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
document.documentElement.setAttribute("data-app-ready", "");

export {};
