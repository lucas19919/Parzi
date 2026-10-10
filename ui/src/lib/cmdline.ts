// Shell-like split for a connector's command line: quotes group words,
// nothing is expanded. "a 'b c' \"\"" -> ["a", "b c", ""].
export function splitLine(text: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: string | null = null;
  let any = false;
  for (const c of text.trim()) {
    if (quote) {
      if (c === quote) quote = null;
      else cur += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      any = true;
    } else if (/\s/.test(c)) {
      if (cur || any) out.push(cur);
      cur = "";
      any = false;
    } else {
      cur += c;
    }
  }
  if (cur || any) out.push(cur);
  return out;
}

// NAME=value per line; blank lines are skipped. A string is an error.
export function parseEnv(text: string): Record<string, string> | string {
  const env: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    const at = line.indexOf("=");
    if (at <= 0) return `"${line}" is not NAME=value`;
    env[line.slice(0, at).trim()] = line.slice(at + 1);
  }
  return env;
}
