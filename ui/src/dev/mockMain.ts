// Entry for mock.html: install the mock backend, then boot the real app.
const { mockView } = await import("./mockTauri");
await import("../main");

if (mockView === "settings") {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: ",", ctrlKey: true }));
}
document.documentElement.setAttribute("data-app-ready", "");

export {};
