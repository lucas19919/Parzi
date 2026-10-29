

function splitWords(text: string): string[] {
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

function quoteToken(token: string): string {
  if (!/[\s"]/.test(token)) return token;
  return `"${token.replace(/"/g, '\\"')}"`;
}

export function formatCommand(binary: string, args: string[]): string {
  return [binary, ...args].filter((part) => part.length > 0).map(quoteToken).join(" ");
}

export function splitCommand(line: string): { binary: string; args: string[] } {
  const [binary = "", ...args] = splitWords(line);
  return { binary, args };
}
