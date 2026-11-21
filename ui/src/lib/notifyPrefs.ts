/** The same notice (digits aside: "attempt 2 of 5") from the same session
 *  shows once in ten minutes (or `within`), however often it repeats. */
const seen = new Map<string, number>();
export function freshNotice(session: string, text: string, now = Date.now(), within = 10 * 60_000): boolean {
  const key = `${session}\u0000${text.replace(/\d+/g, "#")}`;
  const at = seen.get(key);
  if (at !== undefined && now - at < within) return false;
  seen.set(key, now);
  if (seen.size > 200) seen.delete(seen.keys().next().value as string);
  return true;
}
