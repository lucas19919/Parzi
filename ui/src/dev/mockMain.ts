// Entry for mock.html: install the mock backend, then boot the real app.
const { mockView } = await import("./mockTauri");
await import("../main");

const { emit } = await import("@tauri-apps/api/event");
// The app subscribes on mount.
await new Promise((r) => setTimeout(r, 300));

if (mockView === "settings") {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: ",", ctrlKey: true }));
}
if (mockView === "page") {
  // A page asks for the camera.
  await emit("parzi://browser-ask", { kind: "permission", id: "p1", tab: "t-page", origin: "https://meet.google.com", permission: "camera" });
}
// A running agent waits on an approval (Home's recent sessions: needs you).
await emit("parzi://run-event", {
  kind: "approval",
  key: "a1",
  session: "s9",
  call: { id: "c1", name: "shell.exec", args: { cmd: "npm install --save-dev @primer/octicons" }, lane: "build", session: "s9" },
});
// Its toast would come and go while the UI lint audits the page.
await new Promise((r) => setTimeout(r, 50));
(await import("../lib/toast")).toasts.set([]);
document.documentElement.setAttribute("data-app-ready", "");

export {};
