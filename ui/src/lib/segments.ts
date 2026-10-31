
export type Segment =
  | { kind: "md"; body: string }
  | { kind: "widget"; body: unknown }
  | { kind: "artifact"; body: any }
  | { kind: "viz"; body: string };

const segCache = new Map<string, Segment[]>();
const SEG_CACHE_MAX = 400;

export function splitSegments(text: string, cache = true): Segment[] {
  if (cache) {
    const hit = segCache.get(text);
    if (hit !== undefined) return hit;
  }
  const out: Segment[] = [];
  const re = /```(parzi-widget|parzi-artifact|viz)[ \t]*\r?\n([\s\S]*?)```/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push({ kind: "md", body: text.slice(last, m.index) });
    if (m[1] === "viz") {
      out.push({ kind: "viz", body: m[2] });
      last = m.index + m[0].length;
      continue;
    }
    try {
      const payload = JSON.parse(m[2]);
      if (m[1] === "parzi-widget") out.push({ kind: "widget", body: payload });
      else if (payload?.kind === "image") out.push({ kind: "md", body: m[0] });
      else out.push({ kind: "artifact", body: payload });
    } catch {
      out.push({ kind: "md", body: m[0] });
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ kind: "md", body: text.slice(last) });
  if (!cache) return finishUnclosed(out);
  if (segCache.size >= SEG_CACHE_MAX) {
    const first = segCache.keys().next().value;
    if (first !== undefined) segCache.delete(first);
  }
  const finished = finishUnclosed(out);
  segCache.set(text, finished);
  return finished;
}

const UNCLOSED = /```(parzi-widget|parzi-artifact|viz)[ \t]*\r?\n([\s\S]*)$/;

function finishUnclosed(out: Segment[]): Segment[] {
  if (!out.length) return out;
  const tail = out[out.length - 1];
  if (tail.kind !== "md") return out;
  const m = UNCLOSED.exec(tail.body);
  if (!m) return out;
  const head = tail.body.slice(0, m.index);
  const rest: Segment[] = head ? [{ kind: "md", body: head }] : [];
  if (m[1] === "viz") {
    rest.push({ kind: "viz", body: m[2] });
  } else {
    try {
      const payload = JSON.parse(m[2]);
      rest.push(m[1] === "parzi-widget" ? { kind: "widget", body: payload } : { kind: "artifact", body: payload });
    } catch {
      return out;
    }
  }
  return [...out.slice(0, -1), ...rest];
}
