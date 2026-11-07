// A test run read from a command's output, so the thread can show one small
// card (passed, failed, the first failure) instead of a wall of output.
// Only the runners' own summary lines are trusted; anything else is not a
// test run.

export interface TestRun {
  runner: "cargo" | "node" | "jest" | "vitest" | "pytest" | "go";
  passed: number;
  failed: number;
  skipped: number;
  /** What `passed` and `failed` count: tests, or packages for go without -v. */
  unit: "test" | "package";
  /** The names of failed tests, as the runner wrote them. */
  failures: string[];
  /** The first lines of the first failure's message. */
  firstError: string;
}

const MAX_FAILURES = 20;
const ERROR_LINES = 8;

function num(s: string | undefined): number {
  const n = Number(s);
  return Number.isFinite(n) ? n : 0;
}

function sum(text: string, re: RegExp, group = 1): number | null {
  let total: number | null = null;
  for (const m of text.matchAll(re)) total = (total ?? 0) + num(m[group]);
  return total;
}

function names(text: string, re: RegExp): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(re)) {
    const n = m[1].trim();
    if (n && !out.includes(n)) out.push(n);
    if (out.length >= MAX_FAILURES) break;
  }
  return out;
}

function clipLines(lines: string[]): string {
  return lines
    .map((l) => l.replace(/\s+$/, ""))
    .filter((l, i, all) => l || (i > 0 && all[i - 1]))
    .slice(0, ERROR_LINES)
    .join("\n")
    .trim();
}

function cargo(text: string): TestRun | null {
  const re = /^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored/gm;
  if (!/^test result: /m.test(text)) return null;
  const passed = sum(text, re, 1) ?? 0;
  const failed = sum(text, re, 2) ?? 0;
  const skipped = sum(text, re, 3) ?? 0;
  const failures = names(text, /^test (\S+) \.\.\. FAILED$/gm);
  let firstError = "";
  const block = /^---- (\S+) stdout ----\n([\s\S]*?)(?=^---- |^failures:|^test result:|$(?![\s\S]))/m.exec(text);
  if (block) {
    const lines = block[2].split("\n");
    const at = lines.findIndex((l) => /panicked at/.test(l));
    firstError = clipLines(at >= 0 ? lines.slice(at) : lines);
  }
  return { runner: "cargo", passed, failed, skipped, unit: "test", failures, firstError };
}

function node(text: string): TestRun | null {
  const pass = /^# pass (\d+)$/m.exec(text);
  const fail = /^# fail (\d+)$/m.exec(text);
  if (!pass || !fail) return null;
  const skipped = num(/^# skipped (\d+)$/m.exec(text)?.[1]) + num(/^# todo (\d+)$/m.exec(text)?.[1]);
  const failures = names(text, /^\s*not ok \d+ - (.+)$/gm).filter((n) => !/\.(test|spec)\.[cm]?[jt]s$/.test(n));
  let firstError = "";
  const at = text.search(/^\s*not ok \d+ - /m);
  if (at >= 0) {
    const rest = text.slice(at).split("\n").slice(1);
    const err = rest.findIndex((l) => /error:|AssertionError|expected|actual/i.test(l));
    firstError = clipLines(rest.slice(Math.max(0, err)).map((l) => l.trim()));
  }
  return { runner: "node", passed: num(pass[1]), failed: num(fail[1]), skipped, unit: "test", failures, firstError };
}

function jest(text: string): TestRun | null {
  const line = /^Tests:\s+(.+?),?\s*(\d+) total$/m.exec(text);
  if (!line) return null;
  const part = (word: string) => num(new RegExp(`(\\d+) ${word}`).exec(line[1])?.[1]);
  const failures = names(text, /^\s*● (.+?)\s*$/gm).filter((n) => !/^Test suite failed/.test(n));
  let firstError = "";
  const at = text.search(/^\s*● /m);
  if (at >= 0) firstError = clipLines(text.slice(at).split("\n").slice(1).map((l) => l.trim()));
  return { runner: "jest", passed: part("passed"), failed: part("failed"), skipped: part("skipped") + part("todo"), unit: "test", failures, firstError };
}

function vitest(text: string): TestRun | null {
  const line = /^\s*Tests\s+(.+?)\s*\((\d+)\)\s*$/m.exec(text);
  if (!line) return null;
  const part = (word: string) => num(new RegExp(`(\\d+) ${word}`).exec(line[1])?.[1]);
  const failures = names(text, /^\s*(?:FAIL|×)\s+(.+?)\s*$/gm);
  let firstError = "";
  const at = text.search(/^\s*(?:AssertionError|Error|TypeError)\b/m);
  if (at >= 0) firstError = clipLines(text.slice(at).split("\n").map((l) => l.trim()));
  return { runner: "vitest", passed: part("passed"), failed: part("failed"), skipped: part("skipped") + part("todo"), unit: "test", failures, firstError };
}

function pytest(text: string): TestRun | null {
  const line = /^=+ (.*\b(?:passed|failed|error)\b.*) in [\d.]+s.* =+$/m.exec(text);
  if (!line) return null;
  const part = (word: string) => num(new RegExp(`(\\d+) ${word}`).exec(line[1])?.[1]);
  const failures = names(text, /^(?:FAILED|ERROR) (\S+)/gm);
  const why = /^(?:FAILED|ERROR) \S+ - (.+)$/m.exec(text)?.[1] ?? "";
  const err = /^E\s+(.+)$/m.exec(text)?.[1] ?? "";
  return { runner: "pytest", passed: part("passed"), failed: part("failed") + part("errors?"), skipped: part("skipped"), unit: "test", failures, firstError: (err || why).trim() };
}

function go(text: string): TestRun | null {
  const okPkgs = text.match(/^ok\s+\S+/gm)?.length ?? 0;
  const failPkgs = text.match(/^FAIL\s+\S+\s+[\d.]+s/gm)?.length ?? 0;
  if (!okPkgs && !failPkgs) return null;
  const pass = text.match(/^\s*--- PASS: /gm)?.length ?? 0;
  const fail = text.match(/^\s*--- FAIL: /gm)?.length ?? 0;
  const skip = text.match(/^\s*--- SKIP: /gm)?.length ?? 0;
  const verbose = pass + fail > 0;
  const failures = names(text, /^\s*--- FAIL: (\S+)/gm);
  const err = /^\s+\S+_test\.go:\d+: .+$/m.exec(text)?.[0] ?? "";
  return verbose
    ? { runner: "go", passed: pass, failed: fail, skipped: skip, unit: "test", failures, firstError: err.trim() }
    : { runner: "go", passed: okPkgs, failed: failPkgs, skipped: 0, unit: "package", failures, firstError: err.trim() };
}

/** The test run in a command's output, or null when it shows none. */
export function testRunOf(output: string): TestRun | null {
  if (!output) return null;
  const text = output.replace(/\r\n/g, "\n").replace(/\x1b\[[0-9;]*m/g, "");
  return cargo(text) ?? node(text) ?? jest(text) ?? vitest(text) ?? pytest(text) ?? go(text);
}

/** "3 tests failed", "All 116 tests passed", "2 of 5 packages failed". */
export function testHeadline(run: TestRun): string {
  const total = run.passed + run.failed;
  const unit = (n: number) => `${run.unit}${n === 1 ? "" : "s"}`;
  if (run.failed) return total > run.failed ? `${run.failed} of ${total} ${unit(total)} failed` : `${run.failed} ${unit(run.failed)} failed`;
  if (!total) return "No tests ran";
  return total === 1 ? `1 ${run.unit} passed` : `All ${total} ${unit(total)} passed`;
}
