// The viz spec: the JSON an agent writes in a ```viz block (or a Home
// widget note), read leniently into one normalized shape per chart type.
// Pure, so tests pin it down; the components only draw what comes out.
//   { "type": "bars", "title": "Open PRs", "data": [{ "label": "parzi", "value": 4 }] }

export const VIZ_TYPES = ["bars", "line", "hist", "spark", "stat", "progress", "table", "list", "timeline"] as const;
export type VizType = (typeof VIZ_TYPES)[number];

// Status words for dots. Anything else draws a quiet dot.
export type Tone = "ok" | "warn" | "bad" | "accent" | "";

export interface Point {
  label: string;
  value: number;
}

export interface HistBin {
  lo: number;
  hi: number;
  count: number;
}

export interface Series {
  name: string;
  values: (number | null)[];
}

export interface StatItem {
  label: string;
  value: string;
  delta: string;
  // -1 down, 1 up, 0 flat or unknown.
  dir: -1 | 0 | 1;
  spark: number[];
}

export interface ProgressItem {
  label: string;
  // 0 to 1.
  value: number;
  text: string;
}

export interface ListItem {
  label: string;
  detail: string;
  value: string;
  tone: Tone;
  status: string;
}

export interface TimelineItem {
  when: string;
  label: string;
  detail: string;
  tone: Tone;
  status: string;
}

interface Base {
  title: string;
  unit: string;
  // Items beyond what a chart shows (cut to keep it readable).
  more: number;
}

export type VizSpec = Base &
  (
    | { type: "bars"; items: Point[]; orient: "h" | "v" }
    | { type: "hist"; bins: HistBin[]; total: number; mean: number; min: number; max: number }
    | { type: "line"; labels: string[]; series: Series[]; area: boolean; points: boolean }
    | { type: "spark"; values: number[]; area: boolean }
    | { type: "stat"; stats: StatItem[] }
    | { type: "progress"; items: ProgressItem[] }
    | { type: "table"; columns: string[]; rows: string[][]; numeric: boolean[] }
    | { type: "list"; items: ListItem[] }
    | { type: "timeline"; items: TimelineItem[] }
  );

export type Parsed = { ok: true; spec: VizSpec } | { ok: false; error: string; source: string };

// Series beyond the palette fold away rather than reuse a colour.
export const MAX_SERIES = 4;
const MAX_BARS = 40;
const MAX_POINTS = 500;
const MAX_ROWS = 100;
const MAX_COLUMNS = 12;
const MAX_ITEMS = 50;

// The categorical palette: the theme's own accent first, then its link and
// work hues, then its muted grey (validated for adjacent separation on the
// dark surface). Text never wears these; marks do.
export const SERIES_COLORS = ["var(--accent)", "var(--link)", "var(--warn)", "var(--muted)"];

const ALIASES: Record<string, VizType> = {
  bar: "bars",
  bars: "bars",
  barchart: "bars",
  "bar-chart": "bars",
  "chart-bar": "bars",
  hbar: "bars",
  column: "bars",
  columns: "bars",
  hist: "hist",
  histogram: "hist",
  "histogram-chart": "hist",
  distribution: "hist",
  line: "line",
  lines: "line",
  "line-chart": "line",
  "chart-line": "line",
  area: "line",
  spark: "spark",
  sparkline: "spark",
  trend: "spark",
  stat: "stat",
  stats: "stat",
  kpi: "stat",
  metric: "stat",
  metrics: "stat",
  number: "stat",
  progress: "progress",
  meter: "progress",
  meters: "progress",
  gauge: "progress",
  table: "table",
  grid: "table",
  list: "list",
  rows: "list",
  timeline: "timeline",
  events: "timeline",
  history: "timeline",
};

const LABEL_KEYS = ["label", "name", "x", "key", "category", "title", "id"];
const VALUE_KEYS = ["value", "y", "count", "n", "amount", "total", "v"];

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => !!v && typeof v === "object" && !Array.isArray(v);

function num(v: unknown): number | null {
  if (typeof v === "number") return Number.isFinite(v) ? v : null;
  if (typeof v === "string" && v.trim()) {
    const n = Number(v.trim().replace(/,/g, ""));
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

function str(v: unknown, max = 200): string {
  if (v === null || v === undefined) return "";
  const s = typeof v === "string" ? v : typeof v === "number" || typeof v === "boolean" ? String(v) : JSON.stringify(v);
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > max ? `${t.slice(0, max - 1)}…` : t;
}

function pick(o: Obj, keys: string[]): unknown {
  for (const k of keys) if (o[k] !== undefined && o[k] !== null) return o[k];
  return undefined;
}

// A number for people: grouped below 10k, compact above (12.9k, 4.2M), with
// the unit where it reads ($4.2M, 38%, 38 ms).
export function fmt(n: number, unit = ""): string {
  const a = Math.abs(n);
  let s: string;
  if (a >= 1e9) s = `${trim(n / 1e9)}B`;
  else if (a >= 1e6) s = `${trim(n / 1e6)}M`;
  else if (a >= 1e4) s = `${trim(n / 1e3)}k`;
  else if (Number.isInteger(n)) s = n.toLocaleString("en-US");
  else s = (+n.toFixed(a >= 100 ? 1 : 2)).toLocaleString("en-US", { maximumFractionDigits: 2 });
  return withUnit(s, unit);
}

function trim(n: number): string {
  return String(+n.toFixed(Math.abs(n) >= 100 ? 0 : 1));
}

function withUnit(s: string, unit: string): string {
  if (!unit) return s;
  if (/^[$€£¥]$/.test(unit)) return s.startsWith("-") ? `-${unit}${s.slice(1)}` : `${unit}${s}`;
  if (/^(%|x|×|k|°)$/.test(unit)) return `${s}${unit}`;
  return `${s} ${unit}`;
}

// Axis ticks at round numbers covering [lo, hi].
export function niceTicks(lo: number, hi: number, count = 4): number[] {
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return [0, 1];
  if (lo === hi) {
    if (lo === 0) return [0, 1];
    const pad = Math.abs(lo) * 0.5;
    lo = lo > 0 ? 0 : lo - pad;
    hi = hi < 0 ? 0 : hi + pad;
  }
  const raw = (hi - lo) / Math.max(1, count);
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? 10 * mag;
  if (!(step > 0) || !Number.isFinite(step)) return [lo, hi];
  const start = Math.floor(lo / step + 1e-9) * step;
  const out: number[] = [];
  // Each tick from its index, not by adding a step: past 2^53 a small step
  // can be lost to rounding (v + step === v) and a running sum never ends.
  // Rounded-away duplicates are dropped, and a few ticks are plenty.
  for (let i = 0; i <= MAX_TICKS; i++) {
    const v = start + i * step;
    if (v >= hi + step - 1e-9) break;
    const t = +v.toFixed(10);
    if (out[out.length - 1] !== t) out.push(t);
  }
  return out.length >= 2 ? out : [lo, hi];
}

const MAX_TICKS = 50;

// The lower bound a value axis starts at: zero unless the data sits far
// from it (a line of 980 to 1010 would be a flat stripe otherwise).
export function axisFloor(min: number, max: number): number {
  if (min >= 0) return min <= max * 0.5 ? 0 : min;
  return min;
}

function tone(v: unknown): { tone: Tone; status: string } {
  const status = str(v, 40);
  const s = status.toLowerCase();
  if (!s) return { tone: "", status: "" };
  if (/^(ok|done|success|succeeded|pass|passed|passing|green|merged|good|complete|completed|up|healthy)$/.test(s)) return { tone: "ok", status };
  if (/^(warn|warning|pending|waiting|yellow|review|slow|degraded|draft|paused)$/.test(s)) return { tone: "warn", status };
  if (/^(bad|error|fail|failed|failing|red|blocked|down|broken|critical)$/.test(s)) return { tone: "bad", status };
  if (/^(accent|active|running|live|info|open|new|now|current)$/.test(s)) return { tone: "accent", status };
  return { tone: "", status };
}

// Label/value pairs from the shapes agents write: [{label, value}],
// [[label, value]], {label: value}, or numbers with `labels`.
function points(data: unknown, labels: string[]): Point[] {
  const out: Point[] = [];
  if (Array.isArray(data)) {
    data.forEach((d, i) => {
      if (Array.isArray(d) && d.length >= 2) {
        const v = num(d[1]);
        if (v !== null) out.push({ label: str(d[0], 80), value: v });
      } else if (isObj(d)) {
        const v = num(pick(d, VALUE_KEYS));
        if (v !== null) out.push({ label: str(pick(d, LABEL_KEYS), 80) || labels[i] || String(i + 1), value: v });
      } else {
        const v = num(d);
        if (v !== null) out.push({ label: labels[i] ?? String(i + 1), value: v });
      }
    });
  } else if (isObj(data)) {
    for (const [k, raw] of Object.entries(data)) {
      const v = num(raw);
      if (v !== null) out.push({ label: str(k, 80), value: v });
    }
  }
  return out;
}

function strings(v: unknown, max = 80): string[] {
  return Array.isArray(v) ? v.map((x) => str(x, max)) : [];
}

function values(v: unknown): (number | null)[] {
  if (!Array.isArray(v)) return [];
  return v.map((x) => (isObj(x) ? num(pick(x, VALUE_KEYS)) : num(x)));
}

function readBars(o: Obj, data: unknown, kind: string): Partial<VizSpec> | string {
  const items = points(data, strings(o.labels));
  if (!items.length) return 'bars need data like [{"label": "parzi", "value": 4}]';
  const o2 = str(o.orient ?? o.orientation ?? o.direction).toLowerCase();
  const vertical =
    o2.startsWith("v") || o2 === "columns" || o.vertical === true || kind === "column" || kind === "columns" || kind === "histogram";
  const horizontal = o2.startsWith("h") || o.horizontal === true || kind === "hbar";
  // Unset: short labels (days, months) stand as columns, the rest as rows.
  const auto = items.length <= 12 && items.every((p) => p.label.length <= 4) ? "v" : "h";
  const orient = horizontal ? "h" : vertical ? "v" : auto;
  return { type: "bars", items: items.slice(0, MAX_BARS), orient, more: Math.max(0, items.length - MAX_BARS) };
}

function readLine(o: Obj, data: unknown, kind: string): Partial<VizSpec> | string {
  const d = isObj(data) && (data.series || data.values || data.points) ? data : null;
  let labels = strings(d?.labels ?? o.labels);
  let series: Series[] = [];
  const rawSeries = d?.series ?? o.series;
  if (Array.isArray(rawSeries)) {
    rawSeries.forEach((s, i) => {
      if (isObj(s)) {
        const vals = values(s.values ?? s.data ?? s.points);
        if (vals.some((v) => v !== null)) series.push({ name: str(s.name ?? s.label, 60) || `Series ${i + 1}`, values: vals });
      } else if (Array.isArray(s)) {
        const vals = values(s);
        if (vals.some((v) => v !== null)) series.push({ name: `Series ${i + 1}`, values: vals });
      }
    });
  } else {
    const src = d ? (d.values ?? d.points) : data;
    const objs = Array.isArray(src) && src.some((x) => isObj(x) || Array.isArray(x));
    if (objs || isObj(src)) {
      const pts = points(src, labels);
      if (pts.length) {
        labels = pts.map((p) => p.label);
        series = [{ name: str(o.title, 60), values: pts.map((p) => p.value) }];
      }
    } else {
      const vals = values(src);
      if (vals.some((v) => v !== null)) series = [{ name: str(o.title, 60), values: vals }];
    }
  }
  if (!series.length) return 'a line needs data like [3, 5, 4, 8] or {"labels": [...], "series": [{"name": "a", "values": [...]}]}';
  const len = Math.min(MAX_POINTS, Math.max(...series.map((s) => s.values.length)));
  if (len < 2) return "a line needs at least two points";
  const shown = series.slice(0, MAX_SERIES).map((s) => ({ ...s, values: Array.from({ length: len }, (_, i) => s.values[i] ?? null) }));
  labels = Array.from({ length: len }, (_, i) => labels[i] ?? String(i + 1));
  return {
    type: "line",
    labels,
    series: shown,
    area: o.area === true || kind === "area",
    points: o.points === true || o.dots === true,
    more: series.length - shown.length,
  };
}

function readHist(o: Obj, data: unknown): Partial<VizSpec> | string {
  const src = isObj(data) ? (data.values ?? data.points ?? data.data ?? data) : data;
  const vals = (Array.isArray(src) ? src : [])
    .map((x) => (isObj(x) ? num(pick(x, VALUE_KEYS)) : num(x)))
    .filter((v): v is number => v !== null);
  if (!vals.length) return 'a histogram needs numbers, like {"data": [12, 19, 8, 21, 15]}';
  let lo = Math.min(...vals);
  let hi = Math.max(...vals);
  if (lo === hi) {
    const pad = Math.abs(lo) * 0.1 || 1;
    lo -= pad;
    hi += pad;
  }
  const want = num(o.bins ?? o.buckets);
  const target = want !== null ? Math.max(2, Math.min(40, Math.floor(want))) : Math.max(3, Math.min(24, Math.ceil(Math.log2(vals.length)) + 1));
  // Round bin edges like axis ticks; widen the step until they fit the cap.
  const raw = (hi - lo) / Math.max(1, target);
  const mag = 10 ** Math.floor(Math.log10(raw));
  let step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? 10 * mag;
  let e0 = Math.floor(lo / step + 1e-9) * step;
  let nb = Math.max(1, Math.ceil((hi - e0) / step - 1e-9));
  while (nb > 40) {
    step *= 2;
    e0 = Math.floor(lo / step + 1e-9) * step;
    nb = Math.max(1, Math.ceil((hi - e0) / step - 1e-9));
  }
  const bins: HistBin[] = Array.from({ length: nb }, (_, i) => ({ lo: e0 + i * step, hi: e0 + (i + 1) * step, count: 0 }));
  for (const v of vals) {
    const i = Math.max(0, Math.min(nb - 1, Math.floor((v - e0) / step + 1e-9)));
    bins[i].count += 1;
  }
  const mean = vals.reduce((a, b) => a + b, 0) / vals.length;
  return { type: "hist", bins, total: vals.length, mean, min: Math.min(...vals), max: Math.max(...vals), more: 0 };
}

function readSpark(data: unknown): Partial<VizSpec> | string {
  const src = isObj(data) ? (data.values ?? data.points ?? data.data) : data;
  const vals = (Array.isArray(src) && src.some((x) => isObj(x) || Array.isArray(x)) ? points(src, []).map((p) => p.value) : values(src))
    .filter((v): v is number => v !== null)
    .slice(-MAX_POINTS);
  if (vals.length < 2) return "a spark needs at least two numbers, like [3, 5, 4, 8]";
  return { type: "spark", values: vals, area: false };
}

function readStat(o: Obj, data: unknown, unit: string): Partial<VizSpec> | string {
  const one = (d: unknown): StatItem | null => {
    const src: Obj = isObj(d) ? d : { value: d };
    const raw = src.value ?? src.n ?? src.count ?? src.total;
    if (raw === undefined || raw === null || raw === "") return null;
    const n = num(raw);
    const u = str(src.unit, 8) || unit;
    const value = typeof raw === "number" && n !== null ? fmt(n, u) : str(raw, 40);
    const dRaw = src.delta ?? src.change;
    const dn = typeof dRaw === "number" ? dRaw : null;
    const delta = dn !== null ? `${dn > 0 ? "+" : dn < 0 ? "−" : ""}${fmt(Math.abs(dn), u)}` : str(dRaw, 40);
    const dir = dn !== null ? (Math.sign(dn) as -1 | 0 | 1) : /^\+/.test(delta) ? 1 : /^[-−]/.test(delta) ? -1 : 0;
    const note = str(src.vs ?? src.period, 40);
    const spark = values(src.spark ?? src.trend).filter((v): v is number => v !== null).slice(-MAX_POINTS);
    return {
      label: str(src.label ?? src.name ?? src.title, 60),
      value,
      delta: delta && note ? `${delta} ${note}` : delta,
      dir,
      spark: spark.length >= 2 ? spark : [],
    };
  };
  // Top-level fields work too: {"type": "stat", "value": 12, "label": "Open PRs"}.
  const top = { value: o.value, label: o.label, delta: o.delta ?? o.change, vs: o.vs, spark: o.spark ?? o.trend, unit: o.unit };
  const src = data ?? (o.value !== undefined ? top : undefined);
  const list = Array.isArray(src) ? src : [src];
  const stats = list.map(one).filter((s): s is StatItem => !!s);
  if (!stats.length) return 'a stat needs a value, like {"value": 12, "label": "Open PRs"}';
  return { type: "stat", stats: stats.slice(0, 6), more: Math.max(0, stats.length - 6) };
}

function readProgress(o: Obj, data: unknown): Partial<VizSpec> | string {
  const src = data ?? (o.value !== undefined ? o : undefined);
  const list = Array.isArray(src) ? src : [src];
  const items: ProgressItem[] = [];
  for (const d of list) {
    const e: Obj = isObj(d) ? d : { value: d };
    const v = num(e.value ?? e.done ?? e.used);
    if (v === null) continue;
    const max = num(e.max ?? e.total ?? o.max);
    // A fraction (0.42) or a part of max (42 of 100).
    const frac = max !== null && max > 0 ? v / max : v <= 1 ? v : v / 100;
    const f = Math.max(0, Math.min(1, frac));
    const text = max !== null && max > 0 && max !== 100 ? `${fmt(v)} of ${fmt(max)}` : `${Math.round(f * 100)}%`;
    items.push({ label: str(e.label ?? e.name ?? e.title, 60), value: f, text });
  }
  if (!items.length) return 'progress needs a value from 0 to 1, or [{"label": "Tests", "value": 42, "max": 50}]';
  return { type: "progress", items: items.slice(0, 20), more: Math.max(0, items.length - 20) };
}

function readTable(o: Obj, data: unknown): Partial<VizSpec> | string {
  const d = isObj(data) ? data : null;
  let columns = strings(d?.columns ?? d?.headers ?? o.columns ?? o.headers, 60);
  const rawRows = d ? (d.rows ?? d.data) : data;
  if (!Array.isArray(rawRows) || !rawRows.length) return 'a table needs {"columns": [...], "rows": [[...]]} or a list of objects';
  let rows: unknown[][];
  if (rawRows.every(isObj)) {
    if (!columns.length) {
      const keys: string[] = [];
      for (const r of rawRows as Obj[]) for (const k of Object.keys(r)) if (!keys.includes(k)) keys.push(k);
      columns = keys;
    }
    rows = (rawRows as Obj[]).map((r) => columns.map((c) => r[c]));
  } else {
    rows = rawRows.map((r) => (Array.isArray(r) ? r : [r]));
  }
  const width = Math.min(MAX_COLUMNS, Math.max(columns.length, ...rows.map((r) => r.length)));
  const shown = rows.slice(0, MAX_ROWS).map((r) => Array.from({ length: width }, (_, i) => r[i]));
  const numeric = Array.from({ length: width }, (_, i) => {
    const cells = shown.map((r) => r[i]).filter((c) => c !== null && c !== undefined && c !== "");
    return cells.length > 0 && cells.every((c) => typeof c === "number");
  });
  return {
    type: "table",
    columns: Array.from({ length: width }, (_, i) => columns[i] ?? ""),
    // Tables keep exact numbers, grouped; charts are where they round.
    rows: shown.map((r) => r.map((c, i) => (numeric[i] && typeof c === "number" ? c.toLocaleString("en-US", { maximumFractionDigits: 4 }) : str(c, 300)))),
    numeric,
    more: Math.max(0, rows.length - MAX_ROWS),
  };
}

function readList(data: unknown, unit: string): Partial<VizSpec> | string {
  const src = isObj(data) ? Object.entries(data).map(([label, value]) => ({ label, value })) : data;
  if (!Array.isArray(src) || !src.length) return 'a list needs items like [{"label": "Fix the build", "value": "2h", "tone": "ok"}]';
  const items: ListItem[] = src.map((d) => {
    if (!isObj(d)) return { label: str(d, 200), detail: "", value: "", tone: "", status: "" };
    const v = d.value ?? d.meta ?? d.count;
    const n = typeof v === "number" ? v : null;
    return {
      label: str(pick(d, ["label", "title", "name", "text"]), 200),
      detail: str(pick(d, ["detail", "description", "sub", "note", "subtitle"]), 200),
      value: n !== null ? fmt(n, unit) : str(v, 40),
      ...tone(d.tone ?? d.status ?? d.state),
    };
  });
  return { type: "list", items: items.slice(0, MAX_ITEMS), more: Math.max(0, items.length - MAX_ITEMS) };
}

function readTimeline(data: unknown): Partial<VizSpec> | string {
  if (!Array.isArray(data) || !data.length) return 'a timeline needs events like [{"when": "2026-10-01", "label": "Shipped 0.1.27"}]';
  const items: TimelineItem[] = data.map((d) => {
    if (!isObj(d)) return { when: "", label: str(d, 200), detail: "", tone: "", status: "" };
    return {
      when: str(pick(d, ["when", "time", "date", "at", "start"]), 40),
      label: str(pick(d, ["label", "title", "event", "name", "text"]), 200),
      detail: str(pick(d, ["detail", "description", "sub", "note"]), 200),
      ...tone(d.tone ?? d.status ?? d.state),
    };
  });
  return { type: "timeline", items: items.slice(0, MAX_ITEMS), more: Math.max(0, items.length - MAX_ITEMS) };
}

// A spec from a ```viz block's text or an already parsed object. Never
// throws: a spec it can't draw comes back as a short reason.
export function parseViz(input: unknown): Parsed {
  const source = typeof input === "string" ? input : safeJson(input);
  let raw: unknown = input;
  if (typeof input === "string") {
    try {
      raw = JSON.parse(input);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return { ok: false, error: `The viz block isn't valid JSON (${msg.split("\n")[0].slice(0, 120)}).`, source };
    }
  }
  if (!isObj(raw)) return { ok: false, error: 'A viz block is one JSON object, like {"type": "bars", "data": [...]}.', source };
  const kind = str(raw.type ?? raw.kind ?? raw.chart, 40).toLowerCase().replace(/[\s_]+/g, "-");
  const type = ALIASES[kind];
  if (!type) {
    const said = kind ? `"${kind}" isn't a viz type` : "The viz block has no type";
    return { ok: false, error: `${said}. Use one of: ${VIZ_TYPES.join(", ")}.`, source };
  }
  const unit = str(raw.unit, 8);
  const data = raw.data ?? raw.items ?? raw.rows ?? raw.values ?? raw.points ?? raw.events ?? (type === "line" ? raw.series : undefined);
  const tableData = type === "table" && raw.rows !== undefined && raw.data === undefined ? { columns: raw.columns, rows: raw.rows } : data;
  const body =
    type === "bars"
      ? readBars(raw, data, kind)
    : type === "line"
      ? readLine(raw, data, kind)
      : type === "hist"
        ? readHist(raw, data)
        : type === "spark"
          ? readSpark(data)
          : type === "stat"
            ? readStat(raw, raw.data ?? raw.stats ?? raw.items, unit)
            : type === "progress"
              ? readProgress(raw, raw.data ?? raw.items)
              : type === "table"
                ? readTable(raw, tableData)
                : type === "list"
                  ? readList(data, unit)
                  : readTimeline(data);
  if (typeof body === "string") return { ok: false, error: `Can't draw this: ${body}.`, source };
  const spec = { title: str(raw.title, 120), unit, more: 0, ...body } as VizSpec;
  return { ok: true, spec };
}

function safeJson(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2) ?? String(v);
  } catch {
    return String(v);
  }
}

// When an event happened, short: a date reads "Oct 3" (with the year when
// it isn't this one) and a time "Oct 3, 14:05"; anything else as written.
export function whenText(when: string, now = Date.now()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2}))?/.exec(when.trim());
  if (!m) return when;
  const [, y, mo, d, h, mi] = m;
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const name = months[Number(mo) - 1];
  if (!name) return when;
  const year = Number(y) !== new Date(now).getFullYear() ? ` ${y}` : "";
  return `${name} ${Number(d)}${year}${h ? `, ${h}:${mi}` : ""}`;
}

// One line a screen reader hears for a chart.
export function describe(spec: VizSpec): string {
  const name = spec.title ? `${spec.title}: ` : "";
  switch (spec.type) {
    case "bars":
      return `${name}bar chart, ${spec.items.map((p) => `${p.label} ${fmt(p.value, spec.unit)}`).join(", ")}`;
    case "line": {
      const all = spec.series.flatMap((s) => s.values).filter((v): v is number => v !== null);
      const range = all.length ? `, from ${fmt(Math.min(...all), spec.unit)} to ${fmt(Math.max(...all), spec.unit)}` : "";
      const who = spec.series.length > 1 ? `${spec.series.length} series` : "one series";
      return `${name}line chart, ${who} over ${spec.labels.length} points${range}`;
    }
    case "spark": {
      const v = spec.values;
      return `${name}trend of ${v.length} values, last ${fmt(v[v.length - 1], spec.unit)}`;
    }
    case "hist":
      return `${name}histogram of ${spec.total} values, from ${fmt(spec.min, spec.unit)} to ${fmt(spec.max, spec.unit)}, mean ${fmt(spec.mean, spec.unit)}`;
    default:
      return spec.title || spec.type;
  }
}
