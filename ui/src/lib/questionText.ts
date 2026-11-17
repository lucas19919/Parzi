// A question's first line is what is asked. Any lines after it are what the
// person weighs before they answer, such as a plan the agent proposes: a
// numbered or bulleted line is an item of a list ("1. ", "2) ", "• ", "* "),
// "+ " is work added, "- " dropped, "~ " changed, any other line a heading.

export type LineKind = "heading" | "item" | "add" | "drop" | "change";

export interface QuestionLine {
  kind: LineKind;
  text: string;
  /** An item's own number or bullet, as written ("1.", "•"). */
  mark?: string;
}

export interface QuestionParts {
  head: string;
  lines: QuestionLine[];
}

const MARKS: Record<string, LineKind> = { "+": "add", "-": "drop", "~": "change" };
const ITEM = /^(\d{1,3}[.)]|[•*·])\s+(.+)$/;

export function questionParts(text: string): QuestionParts {
  const [head = "", ...rest] = text.split(/\r?\n/);
  const lines: QuestionLine[] = [];
  for (const raw of rest) {
    const line = raw.trim();
    if (!line) continue;
    const item = ITEM.exec(line);
    const kind = MARKS[line[0]];
    if (item) lines.push({ kind: "item", mark: item[1], text: item[2].trim() });
    else if (kind && line[1] === " ") lines.push({ kind, text: line.slice(2).trim() });
    else lines.push({ kind: "heading", text: line });
  }
  return { head: head.trim(), lines };
}

export const LINE_WORDS: Record<LineKind, string> = {
  heading: "",
  item: "",
  add: "Added",
  drop: "Dropped",
  change: "Changed",
};
