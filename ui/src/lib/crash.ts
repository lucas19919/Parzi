
export interface UiLog {
  level: "error" | "warn";
  message: string;
  stack: string;
}

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

export function isNoise(message: string): boolean {
  return /ResizeObserver loop (completed with undelivered notifications|limit exceeded)/.test(message);
}

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
