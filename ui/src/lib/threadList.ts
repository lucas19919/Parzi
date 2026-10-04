export function coalesce(run: () => Promise<void>, waitMs = 100): () => Promise<void> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let waiters: (() => void)[] = [];
  let inFlight = false;
  let lastStart = -Infinity;

  function schedule() {
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
        if (waiters.length) schedule();
      });
  }

  return () =>
    new Promise<void>((resolve) => {
      waiters.push(resolve);
      schedule();
    });
}
