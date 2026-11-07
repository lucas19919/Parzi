// A question's first line is what is asked. Any lines after it are what the
// person weighs before they answer, such as a plan the agent proposes: a
// plain line is a heading, "+ " is work added, "- " dropped, "~ " changed.

export type LineKind = "heading" | "add" | "drop" | "change";

export interface QuestionLine {
  kind: LineKind;
  text: string;
}

export interface QuestionParts {
  head: string;
  lines: QuestionLine[];
}

const MARKS: Record<string, LineKind> = { "+": "add", "-": "drop", "~": "change" };

export function questionParts(text: string): QuestionParts {
  const [head = "", ...rest] = text.split(/\r?\n/);
  const lines: QuestionLine[] = [];
  for (const raw of rest) {
    const line = raw.trim();
    if (!line) continue;
    const kind = MARKS[line[0]];
    if (kind && line[1] === " ") lines.push({ kind, text: line.slice(2).trim() });
    else lines.push({ kind: "heading", text: line });
  }
  return { head: head.trim(), lines };
}

export const LINE_WORDS: Record<LineKind, string> = {
  heading: "",
  add: "Added",
  drop: "Dropped",
  change: "Changed",
};
