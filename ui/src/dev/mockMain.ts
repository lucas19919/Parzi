// Entry for mock.html: install the mock backend, then boot the real app.
const { mockView, mockLive, mockRequest, mockQuestion, mockApproval } = await import("./mockTauri");
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
// The plan panel's agent waits on an approval (its item needs you).
await emit("parzi://run-event", {
  kind: "approval",
  key: "a1",
  session: "s9",
  call: { id: "c1", name: "shell.exec", args: { cmd: "npm install --save-dev @primer/octicons" }, lane: "build", session: "s9", action: "run a command", detail: "npm install --save-dev @primer/octicons", detail_kind: "command", note: "" },
});
if (mockApproval) {
  // The open session's own agent asks.
  await emit("parzi://run-event", {
    kind: "approval",
    key: "a2",
    session: "s1",
    call: { id: "c2", name: "execute", args: { title: "Execute `cargo test -p parzi-core`" }, lane: "build", session: "s1", action: "run a command", detail: "cargo test -p parzi-core", detail_kind: "command", note: "" },
  });
}
if (mockRequest) {
  const asks: Record<string, string> = {
    file: "Send me the crash log from last night's run, so I can see where the queue stalls.",
    image: "Send a screenshot of the composer as it looks on your screen.",
    text: "Paste the error message you saw.",
  };
  await emit("parzi://run-event", { kind: "request", key: "r1", session: "s1", request: asks[mockRequest] ?? asks.text, req_kind: mockRequest });
}
if (mockQuestion) {
  await emit("parzi://run-event", {
    kind: "question",
    key: "q1",
    session: "s1",
    question: "When you pick a branch for a session, what should happen?",
    options: ["Own worktree (recommended)", "Switch the folder"],
    details: [
      "The session works in its own copy of the repo on that branch, so it never touches your checkout or other sessions.",
      "Parzi runs git switch in the project folder: simple, but it changes your checkout for everything working there.",
    ],
    multi: mockQuestion === "multi",
  });
}
// Its toast would come and go while the UI lint audits the page.
await new Promise((r) => setTimeout(r, 50));
(await import("../lib/toast")).toasts.set([]);
if (mockLive) {
  // What the running agents are doing, as their tool calls stream in.
  const LIVE: [string, string[]][] = [
    ["s3", ["Editing D:\\Repos\\Parzi\\ui\\src\\lib\\SidePanel.svelte", "Running `npm run check`", "Reading ui/src/lib/planModel.ts"]],
    ["s6", ["Editing ui/src/lib/SidePanel.svelte", "Running `npm test`"]],
    ["s2", ["Running `cargo test -p parzi-core plan`", "Reading crates/parzi-core/src/plan.rs"]],
  ];
  let beat = 0;
  const tick = () => {
    for (const [session, labels] of LIVE) void emit("parzi://run-event", { kind: "tool_call", session, id: `live-${beat}`, name: "Edit", label: labels[beat % labels.length] });
    beat++;
  };
  tick();
  setInterval(tick, 4000);
}
document.documentElement.setAttribute("data-app-ready", "");

export {};
