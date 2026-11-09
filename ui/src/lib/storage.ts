/** The window's localStorage, never throwing: it can be unavailable (a
 *  private window, blocked site data) and Parzi carries on without it. */

export function readStore(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStore(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {}
}

/** A saved JSON value, or `fallback` when it is missing, unreadable or
 *  fails `valid`. */
export function readJSON<T>(key: string, fallback: T, valid: (v: unknown) => boolean = () => true): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    const v: unknown = JSON.parse(raw);
    return valid(v) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

export function writeJSON(key: string, value: unknown): void {
  writeStore(key, JSON.stringify(value));
}

/** A saved list of strings (folded cards, docked lists…). */
export function readStrings(key: string): string[] {
  const v = readJSON<unknown[]>(key, [], Array.isArray);
  return v.filter((x): x is string => typeof x === "string");
}
