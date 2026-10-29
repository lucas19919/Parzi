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

export function classifyPaste(text: string): "link" | "block" | "program" | "" {
  const t = text.trim();
  if (!t) return "";
  if (/^https?:\/\//i.test(t)) return "link";
  if (t.startsWith("{")) return "block";
  return "program";
}

export function nameFromProgram(line: string): string {
  const words = splitLine(line);
  let file = words[0] ?? "";
  const launcher = /^(npx|npm|node|bun|deno)(\.exe|\.cmd)?$/i.test(file.split(/[\\/]/).pop() ?? "");
  if (launcher) {
    const pkg = words.find((w, i) => i > 0 && !w.startsWith("-"));
    if (pkg) file = pkg;
  }
  const leaf = (file.split(/[\\/]/).pop() ?? "").replace(/\.(exe|cmd|bat|js|mjs|cjs)$/i, "");
  let id = leaf.replace(/[^a-z0-9_-]/gi, "").toLowerCase();
  if (!/^[a-z0-9]/.test(id)) id = `app${id}`;
  id = id.slice(0, 32);
  return /^[a-z0-9][a-z0-9_-]{0,31}$/.test(id) ? id : "app";
}

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
