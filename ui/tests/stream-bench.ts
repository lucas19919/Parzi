/** Streaming-path benchmark (efficiency.md E4, §3.4).
 *
 * Feeds one deterministic 4 000-token answer with three code fences through
 * the exact functions Thread.svelte calls, and counts the work each path does:
 *
 *   old  — one reactive assignment per delta, splitSegments(whole buffer) +
 *          renderMarkdown(every segment) per delta. What HEAD did.
 *   coal — the 60 ms flush only (still re-parses the whole buffer per flush).
 *   new  — 60 ms flush + LiveMarkdown: finished blocks parsed once, only the
 *          block being written is re-parsed.
 *
 * Run: npm run bench:stream   (node >= 22 strips the types itself)
 * No DOM here, so DOMPurify passes the HTML through; markdown-it + highlight.js
 * are the measured cost and they are the quadratic term.
 */
import {
  renderMarkdown,
  splitSegments,
  LiveMarkdown,
  mdStats,
  resetMdStats,
} from "../src/lib/md.ts";

/** Deterministic word source: no Math.random, same run every time. */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}

const WORDS =
  "the run store emits one text delta per chunk which the host forwards to the webview where markdown is parsed sanitized and highlighted before the compositor paints it again".split(
    " ",
  );

/** ~4 000 whitespace tokens of prose, headings, a list and three fences. */
function answer(): string {
  const r = rng(7);
  const out: string[] = [];
  let tokens = 0;
  const words = (n: number) => {
    const w: string[] = [];
    for (let i = 0; i < n; i++) w.push(WORDS[Math.floor(r() * WORDS.length)]);
    tokens += n;
    return w.join(" ");
  };
  const fences = [
    ["rust", (i: number) => `fn lane_${i}(events: &[Event]) -> usize {\n    events.iter().filter(|e| e.kind == Kind::Text).count()\n}`],
    ["python", (i: number) => `def lane_${i}(events):\n    return sum(1 for e in events if e.kind == "text")`],
    ["json", (i: number) => `{\n  "lane": ${i},\n  "flush_ms": 60,\n  "fences": 3\n}`],
  ] as const;
  let section = 0;
  while (tokens < 4000) {
    out.push(`## ${words(4)}`);
    out.push(words(60));
    out.push(words(45));
    out.push(["- " + words(8), "- " + words(9), "- " + words(7)].join("\n"));
    if (section < 3) {
      const [lang, body] = fences[section];
      const block = Array.from({ length: 12 }, (_, i) => body(section * 12 + i)).join("\n");
      out.push("```" + lang + "\n" + block + "\n```");
      tokens += Math.ceil(block.length / 4);
    }
    section++;
  }
  return out.join("\n\n");
}

const TEXT = answer();
const TOKENS: string[] = TEXT.split(/(?<=\s)/).filter((t) => t.length);
/** 50 tok/s is a fast provider; 60 ms of that is three deltas per flush. */
const PER_FLUSH = 3;

interface Row {
  name: string;
  ms: number;
  parses: number;
  parsedChars: number;
  splits: number;
  frames: number;
}

function measure(name: string, fn: (emit: (buf: string) => void) => void): Row {
  resetMdStats();
  let frames = 0;
  let buf = "";
  const t0 = performance.now();
  fn((b) => {
    buf = b;
    frames++;
  });
  const ms = performance.now() - t0;
  if (!buf.length) throw new Error("no output");
  return {
    name,
    ms,
    parses: mdStats.parses,
    parsedChars: mdStats.parsedChars,
    splits: mdStats.splits,
    frames,
  };
}

type Path = (emit: (buf: string) => void) => void;

/** HEAD: every delta re-splits and re-renders the whole buffer, through the
    caches — which is also what evicts every finished message (probe below). */
const oldPath: Path = (emit) => {
  let live = "";
  for (const tok of TOKENS) {
    live += tok;
    let html = "";
    for (const seg of splitSegments(live)) {
      if (seg.kind === "md") html += renderMarkdown(seg.body);
    }
    emit(html);
  }
};

/** Coalescing alone: the same work, 3× fewer times. */
const coalPath: Path = (emit) => {
  let live = "";
  for (let i = 0; i < TOKENS.length; i++) {
    live += TOKENS[i];
    if (i % PER_FLUSH !== PER_FLUSH - 1 && i !== TOKENS.length - 1) continue;
    let html = "";
    for (const seg of splitSegments(live, false)) {
      if (seg.kind === "md") html += renderMarkdown(seg.body, false);
    }
    emit(html);
  }
};

/** This lane: coalescing + incremental tail, exactly as Thread.svelte does it. */
const newPath: Path = (emit) => {
  const lm = new LiveMarkdown();
  let live = "";
  for (let i = 0; i < TOKENS.length; i++) {
    live += TOKENS[i];
    if (i % PER_FLUSH !== PER_FLUSH - 1 && i !== TOKENS.length - 1) continue;
    let html = "";
    const segs = splitSegments(live, false);
    for (let s = 0; s < segs.length; s++) {
      const seg = segs[s];
      if (seg.kind !== "md") continue;
      if (s === segs.length - 1) {
        const part = lm.render(String(seg.body));
        html += part.head + part.tail;
      } else {
        html += renderMarkdown(seg.body);
      }
    }
    emit(html);
  }
};

// Warm the JIT so the first path measured is not also paying for compilation.
for (let i = 0; i < 40; i++) renderMarkdown(`warmup ${i}\n\n\`\`\`json\n{"i": ${i}}\n\`\`\``);

const old = measure("old (per delta)", oldPath);
const coalesced = measure("coalesced 60 ms", coalPath);
const fresh = measure("coalesced + incremental", newPath);

const n = TOKENS.length;
const cols = ["path", "ms", "frames", "splits", "md parses", "parsed chars", "chars/token", "ms/token"];
const rows = [old, coalesced, fresh].map((r) => [
  r.name,
  r.ms.toFixed(0),
  String(r.frames),
  String(r.splits),
  String(r.parses),
  r.parsedChars.toLocaleString("en-US"),
  (r.parsedChars / n).toFixed(0),
  (r.ms / n).toFixed(3),
]);
const w = cols.map((c, i) => Math.max(c.length, ...rows.map((r) => r[i].length)));
const line = (r: string[]) => r.map((c, i) => c.padEnd(w[i])).join("  ");
console.log(`tokens ${n}, chars ${TEXT.length}, fences 3, flush ${PER_FLUSH} deltas`);
console.log(line(cols));
console.log(w.map((x) => "-".repeat(x)).join("  "));
for (const r of rows) console.log(line(r));
console.log(
  `\nold -> new: ms ${(old.ms / fresh.ms).toFixed(1)}x, parses ${(old.parses / fresh.parses).toFixed(1)}x, parsed chars ${(old.parsedChars / fresh.parsedChars).toFixed(1)}x`,
);

/** Point 4: a transcript of finished messages must still come out of the LRU
    in O(1) after a long answer has streamed past it. The old path writes one
    cache entry per delta, so 4 000 deltas evict all 400 slots. */
function evictionProbe(label: string, path: Path): void {
  const msgs = Array.from(
    { length: 24 },
    (_, i) => `## finished message ${i}\n\n${WORDS.join(" ")}\n\n\`\`\`rust\nfn m${i}() {}\n\`\`\``,
  );
  for (const m of msgs) for (const s of splitSegments(m)) if (s.kind === "md") renderMarkdown(s.body);
  path(() => {});
  resetMdStats();
  const t = performance.now();
  for (const m of msgs) for (const s of splitSegments(m)) if (s.kind === "md") renderMarkdown(s.body);
  const ms = performance.now() - t;
  console.log(
    `${label.padEnd(23)} re-render of 24 finished messages after the stream: ${mdStats.parses} parses, ${mdStats.hits} hits, ${ms.toFixed(2)} ms`,
  );
  if (label.startsWith("coalesced + incr") && mdStats.parses !== 0) {
    console.error("FAIL: a finished message re-parsed after streaming (cache was evicted)");
    process.exitCode = 1;
  }
}
/** The incremental tail must render the same content as one whole-buffer
    parse: same visible text, same fences, nothing dropped at a block seam. */
function fidelityCheck(label: string, doc: string): void {
  const lm = new LiveMarkdown();
  const toks = doc.split(/(?<=\s)/).filter((t) => t.length);
  let live = "";
  let html = "";
  for (let i = 0; i < toks.length; i++) {
    live += toks[i];
    if (i % PER_FLUSH !== PER_FLUSH - 1 && i !== toks.length - 1) continue;
    const p = lm.render(live);
    html = p.head + p.tail;
  }
  const whole = renderMarkdown(doc, false);
  const text = (h: string) => h.replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim();
  const fences = (h: string) => (h.match(/class="codeblock/g) || []).length;
  const ok = text(html) === text(whole) && fences(html) === fences(whole);
  console.log(
    `  ${label.padEnd(26)} text ${ok ? "identical" : "DIFFERS"}, fences ${fences(html)}/${fences(whole)}, blocks ${(html.match(/<\/p>/g) || []).length}/${(whole.match(/<\/p>/g) || []).length}`,
  );
  if (!ok) {
    console.error(`FAIL: the incremental tail does not match a whole-buffer render (${label})`);
    process.exitCode = 1;
  }
}
console.log("\nfidelity: incremental tail vs one whole-buffer parse");
fidelityCheck("4 000-token answer", TEXT);
fidelityCheck(
  "fence with blank lines",
  "intro line\n\n```rust\nfn a() {\n\n    // ## not a heading\n\n}\n```\n\nafter the fence\n",
);
fidelityCheck(
  "loose list then prose",
  "- one\n\n- two\n\n- three\n\nand a closing paragraph about the list above\n",
);
fidelityCheck(
  "table then prose",
  "| a | b |\n| - | - |\n| 1 | 2 |\n\nthe table above has two rows\n\nand one more paragraph\n",
);
fidelityCheck("quote then prose", "> quoted\n>\n> still quoted\n\nplain again\n");

console.log("");
evictionProbe("old (per delta)", oldPath);
evictionProbe("coalesced + incremental", newPath);
