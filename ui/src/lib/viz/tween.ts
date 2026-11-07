// The numbers a chart animates, taken out of a spec and put back, so one
// tween in Viz.svelte moves every kind of chart: bars, histograms and
// progress grow from nothing on entry, and every chart eases from its old
// values to new ones when an agent updates it.
import type { VizSpec } from "./spec.ts";

/** The spec's moving numbers, in a fixed order. Lines keep nulls out. */
export function numbersOf(s: VizSpec): number[] {
  switch (s.type) {
    case "bars":
      return s.items.map((p) => p.value);
    case "hist":
      return s.bins.map((b) => b.count);
    case "line":
      return s.series.flatMap((x) => x.values.filter((v): v is number => v !== null));
    case "spark":
      return s.values;
    case "stat":
      return s.stats.flatMap((x) => x.spark);
    case "progress":
      return s.items.map((p) => p.value);
    default:
      return [];
  }
}

/** The spec with `nums` put back in. `grow` (0 to 1) counts stat numbers
 *  and progress up on entry; bars and histograms take `grow` themselves,
 *  since their scale must stay that of the real values, and lines and
 *  sparks draw on. */
export function withNumbers(s: VizSpec, nums: number[], grow = 1): VizSpec {
  let i = 0;
  const next = (fallback: number) => (i < nums.length ? nums[i++] : fallback);
  switch (s.type) {
    case "bars":
      return { ...s, items: s.items.map((p) => ({ ...p, value: next(p.value) })) };
    case "hist":
      return { ...s, bins: s.bins.map((b) => ({ ...b, count: next(b.count) })) };
    case "line":
      return { ...s, series: s.series.map((x) => ({ ...x, values: x.values.map((v) => (v === null ? null : next(v))) })) };
    case "spark":
      return { ...s, values: s.values.map((v) => next(v)) };
    case "stat":
      return { ...s, stats: s.stats.map((x) => ({ ...x, value: countText(x.value, grow), spark: x.spark.map((v) => next(v)) })) };
    case "progress":
      return { ...s, items: s.items.map((p) => ({ ...p, value: next(p.value) * grow, text: countText(p.text, grow) })) };
    default:
      return s;
  }
}

/** What a chart is apart from its numbers. When this stays the same an
 *  update eases from the old values; when it changes the chart is new. */
export function shapeOf(s: VizSpec): string {
  return JSON.stringify(withNumbers(s, numbersOf(s).map(() => 0)));
}

function decimals(n: number): number {
  if (!Number.isFinite(n) || Number.isInteger(n)) return 0;
  const t = String(n);
  const e = t.indexOf("e-");
  if (e >= 0) return Math.min(6, Number(t.slice(e + 2)));
  return Math.min(6, (t.split(".")[1] ?? "").length);
}

/** Between `a` and `b` at `t`, each value rounded as finely as its target,
 *  so whole numbers count up as whole numbers. A longer `b` starts its new
 *  values from 0. */
export function lerpNumbers(a: number[], b: number[], t: number): number[] {
  return b.map((to, i) => {
    const from = a[i] ?? 0;
    const p = 10 ** decimals(to);
    return Math.round((from + (to - from) * t) * p) / p;
  });
}

/** A stat's text with its number counted to `t` of the way: "1,284 users"
 *  at 0.5 is "642 users". Text with no number, or t at 1, is unchanged. */
export function countText(raw: string, t: number): string {
  if (t >= 1) return raw;
  const m = /-?\d[\d,]*(?:\.\d+)?/.exec(raw);
  if (!m) return raw;
  const text = m[0];
  const value = Number(text.replace(/,/g, ""));
  if (!Number.isFinite(value)) return raw;
  const places = (text.split(".")[1] ?? "").length;
  const now = Math.max(0, t) * value;
  let out = now.toFixed(places);
  if (text.includes(",")) {
    const [whole, frac] = out.split(".");
    out = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",") + (frac ? `.${frac}` : "");
  }
  return raw.slice(0, m.index) + out + raw.slice(m.index + text.length);
}
