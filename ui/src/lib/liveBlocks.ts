/** Where a parsed markdown token starts, as markdown-it reports it. */
export interface BlockToken {
  level: number;
  nesting: number;
  map: [number, number] | null;
}

/** A reply still streaming, cut into its top-level markdown blocks (a
 *  paragraph, a list, a table, a code block…) from the tokens of the whole
 *  text, so a list or table stays one block however it grows. */
export function splitBlocks(text: string, tokens: BlockToken[]): string[] {
  const lines = text.split("\n");
  const starts: number[] = [];
  for (const t of tokens) {
    if (t.level === 0 && t.map && t.nesting !== -1 && t.map[0] !== starts[starts.length - 1]) starts.push(t.map[0]);
  }
  if (!starts.length) return text.trim() ? [text] : [];
  starts[0] = 0;
  return starts.map((from, i) => lines.slice(from, starts[i + 1] ?? lines.length).join("\n")).filter((b) => b.trim());
}
