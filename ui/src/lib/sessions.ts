let clearer: (() => Promise<void>) | null = null;

export function provideClearSessions(fn: () => Promise<void>): () => void {
  clearer = fn;
  return () => {
    if (clearer === fn) clearer = null;
  };
}

export async function clearFinishedSessions(): Promise<void> {
  if (!clearer) throw new Error("Sessions aren't ready yet");
  await clearer();
}
