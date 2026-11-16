// Messages from before project plans became drafts can carry quoted plan
// items as <plan-item> blocks. They are history now: this takes them out of
// the text and keeps their titles, so an old message still reads cleanly.

const BLOCK = /\n*<plan-item\b([^>]*)>[\s\S]*?<\/plan-item>\n*/g;

function unescapeAttr(v: string): string {
  return v.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

export function splitQuotes(text: string): { text: string; titles: string[] } {
  const titles: string[] = [];
  const rest = text.replace(BLOCK, (_, attrs: string) => {
    const a: Record<string, string> = {};
    for (const m of attrs.matchAll(/([a-z_]+)="([^"]*)"/g)) a[m[1]] = unescapeAttr(m[2]);
    titles.push(a.title || a.id || "Plan item");
    return "\n\n";
  });
  return { text: titles.length ? rest.trim() : text, titles };
}
