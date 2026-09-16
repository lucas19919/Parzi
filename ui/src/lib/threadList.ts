/**
 * Trailing-edge coalescer (E7).
 *
 * A run emits hundreds of events; the sidebar only needs the newest list
 * once per window. `coalesce` folds any number of callers into at most one
 * in-flight `run()` per `waitMs`, and hands every caller a promise that
 * resolves once the run which covers its call has finished — so callers
 * that `await` it still read fresh data afterwards.
 */
export function coalesce(run: () => Promise<void>, waitMs = 100): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let waiters: (() => void)[] = [];
  let inFlight = false;
  let lastStart = -Infinity;

  function schedule() {
    // One pending timer, one live run: everything else is folded in.
    if (timer !== null || inFlight) return;
    const delay = Math.max(0, waitMs - (Date.now() - lastStart));
    timer = setTimeout(fire, delay);
  }

  function fire() {
    timer = null;
    lastStart = Date.now();
    inFlight = true;
    const woken = waiters;
    waiters = [];
    void Promise.resolve()
      .then(run)
      .catch(() => {})
      .then(() => {
        inFlight = false;
        for (const w of woken) w();
        // Calls that landed mid-flight get the next window, never a nested run.
        if (waiters.length) schedule();
      });
  }

  return () =>
    new Promise<void>((resolve) => {
      waiters.push(resolve);
      schedule();
    });
}

/**
 * Which run event can change a row in the sidebar (E7).
 *
 * Text and reasoning deltas, usage ticks and tool status all render from
 * state the UI already holds, so they must not trigger a `list_threads`.
 * What does: a session ending, a notice (agents spawn/message each other,
 * which renames rows), and the first event seen for a session, which is the
 * queued → running transition the row shows.
 */
export function changesThreadList(kind: string, firstForSession: boolean): boolean {
  return kind === "done" || kind === "error" || kind === "notice" || firstForSession;
}
