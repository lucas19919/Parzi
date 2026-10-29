const BLOCK = /\n*<plan-item\b([^>]*)>[\s\S]*?<\/plan-item>\n*/g;

export function hidePlanItems(text: string): { text: string; items: string[] } {
  const items: string[] = [];
  const rest = text.replace(BLOCK, (_, attrs: string) => {
    const title = /\btitle="([^"]*)"/.exec(attrs)?.[1] ?? /\bid="([^"]*)"/.exec(attrs)?.[1] ?? "";
    items.push(title.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&") || "Plan item");
    return "\n\n";
  });
  return { text: items.length ? rest.trim() : text, items };
}
