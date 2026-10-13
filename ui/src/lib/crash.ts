// Errors that reach the window. In Svelte 4 an exception inside an update
// leaves the scheduler stuck (its "update scheduled" flag never resets),
// so the window stops redrawing until a reload while the backend answers
// fine. Each error goes to the desk's log (ui_log), rate limited, and a
// real exception brings up one quiet notice with a Reload button. The
// notice is plain DOM on purpose: a stuck Svelte can't draw anything.

export interface UiLog {
  level: "error" | "warn";
  message: string;
  stack: string;
}

// What an error event or a rejection carries, as text. Rejections that
// aren't Errors (a backend command's message) are warnings: they don't
// break drawing.
export function describeError(reason: unknown): UiLog {
  if (reason instanceof Error) {
    return { level: "error", message: `${reason.name}: ${reason.message}`.slice(0, 2000), stack: (reason.stack ?? "").slice(0, 8000) };
  }
  let text: string;
  try {
    text = typeof reason === "string" ? reason : JSON.stringify(reason) ?? String(reason);
  } catch {
    text = String(reason);
  }
  return { level: "warn", message: text.slice(0, 2000), stack: "" };
}

// Messages the browser sends that aren't faults (a ResizeObserver that
// took two frames to settle).
export function isNoise(message: string): boolean {
  return /ResizeObserver loop (completed with undelivered notifications|limit exceeded)/.test(message);
}

// At most `limit` reports in any `windowMs`: a loop that throws on every
// frame doesn't flood the log. `sent` holds the times of recent reports.
export function allowReport(sent: number[], now: number, limit = 5, windowMs = 60_000): boolean {
  while (sent.length && now - sent[0] >= windowMs) sent.shift();
  if (sent.length >= limit) return false;
  sent.push(now);
  return true;
}

let notice: HTMLElement | null = null;

function showNotice() {
  if (notice?.isConnected) return;
  const box = document.createElement("div");
  box.className = "crash-note";
  box.setAttribute("role", "alert");
  // A web page tab's native view covers the bottom corner: the notice
  // goes where the toasts go then, into the top bar.
  if (document.querySelector(".toasts.chrome")) box.classList.add("chrome");
  const text = document.createElement("span");
  text.textContent = "Something went wrong.";
  const reload = document.createElement("button");
  reload.type = "button";
  reload.textContent = "Reload";
  reload.addEventListener("click", () => location.reload());
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", () => box.remove());
  box.append(text, reload, dismiss);
  document.body.appendChild(box);
  notice = box;
}

// Listens for errors and unhandled rejections on the window. `send`
// delivers a report (to the backend); it may fail or not exist yet.
export function installCrashHandlers(send: (log: UiLog) => Promise<unknown>): () => void {
  const sent: number[] = [];
  const report = (reason: unknown, fallback = "") => {
    const log = describeError(reason ?? fallback);
    if (!log.message && fallback) log.message = fallback;
    if (isNoise(log.message)) return;
    if (allowReport(sent, Date.now())) {
      try {
        void send(log).catch(() => {});
      } catch {
        // No backend (yet): the notice still shows.
      }
    }
    if (log.level === "error") showNotice();
  };
  const onError = (e: ErrorEvent) => report(e.error, e.message);
  const onRejection = (e: PromiseRejectionEvent) => report(e.reason);
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onRejection);
  };
}
