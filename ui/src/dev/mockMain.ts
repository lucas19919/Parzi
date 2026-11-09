const { mockView, mockLive, mockRequest, mockQuestion, mockApproval } = await import("./mockTauri");
await import("../main");

const { emit } = await import("@tauri-apps/api/event");
await new Promise((r) => setTimeout(r, 300));

// ?bg=1 paints a wallpaper on a canvas (the mock has no files to serve).
if (new URLSearchParams(location.search).get("bg")) {
  const c = document.createElement("canvas");
  c.width = 1600;
  c.height = 1000;
  const g = c.getContext("2d")!;
  const sky = g.createLinearGradient(0, 0, 0, c.height);
  sky.addColorStop(0, "#1d3b6b");
  sky.addColorStop(0.55, "#d9825b");
  sky.addColorStop(1, "#2a1a2e");
  g.fillStyle = sky;
  g.fillRect(0, 0, c.width, c.height);
  g.fillStyle = "#f6d28b";
  g.beginPath();
  g.arc(1080, 560, 120, 0, Math.PI * 2);
  g.fill();
  for (const [y, color] of [[640, "#3b2a4a"], [720, "#2a2038"], [820, "#1b1526"]] as const) {
    g.fillStyle = color;
    g.beginPath();
    g.moveTo(0, y);
    for (let x = 0; x <= c.width; x += 80) g.lineTo(x, y - 60 * Math.sin(x / 210 + y));
    g.lineTo(c.width, c.height);
    g.lineTo(0, c.height);
    g.fill();
  }
  // ?bg=pale: a bright day sky over a red field, the hard case for white text.
  if (new URLSearchParams(location.search).get("bg") === "pale") {
    const day = g.createLinearGradient(0, 0, 0, c.height);
    day.addColorStop(0, "#7f9cc4");
    day.addColorStop(0.45, "#dfe8f2");
    day.addColorStop(0.7, "#f2f4f7");
    g.fillStyle = day;
    g.fillRect(0, 0, c.width, c.height);
    g.fillStyle = "rgba(255,255,255,0.85)";
    for (const [x, y, r] of [[260, 260, 90], [360, 240, 70], [1150, 200, 110], [1270, 230, 80], [700, 420, 60]]) {
      g.beginPath();
      g.arc(x, y, r, 0, Math.PI * 2);
      g.fill();
    }
    g.fillStyle = "#c8394a";
    g.beginPath();
    g.moveTo(0, 820);
    g.lineTo(c.width, 700);
    g.lineTo(c.width, c.height);
    g.lineTo(0, c.height);
    g.fill();
  }
  document.dispatchEvent(new CustomEvent("parzi:bg", { detail: c.toDataURL("image/jpeg", 0.85) }));
}

if (mockView === "settings") {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: ",", ctrlKey: true }));
}
if (mockView === "manager") {
  document.querySelector<HTMLElement>(".topbar .manager-btn")?.click();
}
if (mockView === "page") {
  await emit("parzi://browser-ask", { kind: "permission", id: "p1", tab: "t-page", origin: "https://meet.google.com", permission: "camera" });
}
await emit("parzi://run-event", {
  kind: "approval",
  key: "a1",
  session: "s9",
  call: { id: "c1", name: "shell.exec", args: { cmd: "npm install --save-dev @primer/octicons" }, session: "s9", action: "run a command", detail: "npm install --save-dev @primer/octicons", detail_kind: "command", note: "" },
});
if (mockApproval) {
  await emit("parzi://run-event", {
    kind: "approval",
    key: "a2",
    session: "s1",
    call: { id: "c2", name: "execute", args: { title: "Execute `cargo test -p parzi-core`" }, session: "s1", action: "run a command", detail: "cargo test -p parzi-core", detail_kind: "command", note: "" },
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
if (mockQuestion === "plan") {
  await emit("parzi://run-event", {
    kind: "question",
    key: "q1",
    session: "s1",
    question: 'Change the plan "0.1.28"?\nGoal: A calmer sidebar driven by plans\n~ Short session names (was Session names)\n~ Plan panel waits for names\n- Work view (dropped)\n+ Plan history (Next)\n+ Plan sign-off (Now)',
    options: ["Use this plan", "Not now"],
    details: ["Save it. Agents in this project work from it.", "Nothing is saved. The agent carries on without it."],
    multi: false,
  });
} else if (mockQuestion) {
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
await new Promise((r) => setTimeout(r, 50));
(await import("../lib/toast")).toasts.set([]);
if (mockLive) {
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
// ?stream=1 streams a reply into the open session (view=session), in the
// uneven bursts real agents send: prose, a list, a long code block, a table,
// a step in between, then the end of the turn.
if (new URLSearchParams(location.search).get("stream")) {
  const code = Array.from({ length: 40 }, (_, i) => `  const step${i} = measure(el, ${i}); // line ${i + 1}`).join("\n");
  const first = "I'll check how the composer measures itself.";
  const reply = [
    "The freeze comes from **autosize** measuring the whole textarea on every key. Here is what changes:",
    "",
    "1. Measure once per frame, not per key.",
    "2. Cache the line height, which never changes while typing.",
    "3. Skip the measure entirely when the text did not wrap.",
    "",
    "```ts",
    "export function autosize(el: HTMLTextAreaElement) {",
    code,
    "}",
    "```",
    "",
    "| Case | Before | After |",
    "| --- | ---: | ---: |",
    "| 1 line | 4 ms | 0.2 ms |",
    "| 300 lines | 61 ms | 0.4 ms |",
    "",
    "That keeps typing smooth even in very long drafts. I also added a test that types 300 lines and checks no frame takes longer than 8 ms.",
  ].join("\n");
  const sid = "s1";
  const burst = async (text: string) => {
    let i = 0;
    while (i < text.length) {
      const n = 4 + Math.floor(Math.random() * 40);
      await emit("parzi://run-event", { kind: "text", session: sid, text: text.slice(i, i + n) });
      i += n;
      await new Promise((r) => setTimeout(r, 20 + Math.random() * 40));
    }
  };
  (window as unknown as { __stream: Promise<void> }).__stream = (async () => {
    await new Promise((r) => setTimeout(r, 400));
    await burst(first);
    await emit("parzi://run-event", { kind: "tool_call", session: sid, id: "st1", name: "Read", label: "Reading ui/src/lib/autosize.ts" });
    await new Promise((r) => setTimeout(r, 300));
    await emit("parzi://run-event", { kind: "tool_result", session: sid, id: "st1", name: "Read", ok: true, ms: 300 });
    await burst(reply);
    await new Promise((r) => setTimeout(r, 300));
    (globalThis as { __mockSaved?: unknown[] }).__mockSaved = [
      { kind: "user", text: "Why does typing stall?" },
      { kind: "assistant", text: first, done: true },
      { kind: "tool_call", id: "st1", name: "Read", label: "Reading ui/src/lib/autosize.ts", args: {} },
      { kind: "tool_result", id: "st1", name: "Read", ok: true, ms: 300, output: "" },
      { kind: "assistant", text: reply, done: true },
    ];
    await emit("parzi://run-event", { kind: "done", session: sid });
  })();
}
document.documentElement.setAttribute("data-app-ready", "");

export {};
