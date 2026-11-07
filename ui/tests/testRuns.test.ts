import { test } from "node:test";
import assert from "node:assert/strict";
import { testHeadline, testRunOf } from "../src/lib/testRuns.ts";

test("cargo: every binary's results add up, with the first panic", () => {
  const out = `exit 101
running 3 tests
test plan::a ... ok
test plan::b ... FAILED
test plan::c ... ignored

failures:

---- plan::b stdout ----

thread 'plan::b' (123) panicked at crates/parzi-core/src/plan.rs:42:9:
assertion \`left == right\` failed
  left: 1
 right: 2
note: run with \`RUST_BACKTRACE=1\` environment variable to display a backtrace

failures:
    plan::b

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
`;
  const run = testRunOf(out)!;
  assert.equal(run.runner, "cargo");
  assert.deepEqual([run.passed, run.failed, run.skipped], [5, 1, 1]);
  assert.deepEqual(run.failures, ["plan::b"]);
  assert.match(run.firstError, /^thread 'plan::b' \(123\) panicked at crates\/parzi-core\/src\/plan.rs:42:9:\nassertion/);
  assert.equal(testHeadline(run), "1 of 6 tests failed");
});

test("node's test runner reads its totals and the failing test", () => {
  const out = `TAP version 13
# Subtest: arrows wrap
not ok 3 - arrows wrap
  ---
  error: 'Expected values to be strictly equal:\\n\\n"docs" !== "docs.rs"'
  ...
# tests 12
# pass 11
# fail 1
# skipped 0
# todo 0
`;
  const run = testRunOf(out)!;
  assert.equal(run.runner, "node");
  assert.deepEqual([run.passed, run.failed], [11, 1]);
  assert.deepEqual(run.failures, ["arrows wrap"]);
  assert.match(run.firstError, /^error: 'Expected values/);
});

test("jest, vitest and pytest summaries", () => {
  const jest = testRunOf(`  ● Cart › adds an item\n\n    expect(received).toBe(expected)\n\nTests:       1 failed, 2 skipped, 45 passed, 48 total\n`)!;
  assert.deepEqual([jest.runner, jest.passed, jest.failed, jest.skipped], ["jest", 45, 1, 2]);
  assert.deepEqual(jest.failures, ["Cart › adds an item"]);

  const vitest = testRunOf(` × src/cart.test.ts > adds an item\nAssertionError: expected 1 to be 2\n Test Files  1 failed | 3 passed (4)\n      Tests  1 failed | 44 passed (45)\n`)!;
  assert.deepEqual([vitest.runner, vitest.passed, vitest.failed], ["vitest", 44, 1]);
  assert.equal(vitest.firstError.split("\n")[0], "AssertionError: expected 1 to be 2");

  const py = testRunOf(`E       assert 1 == 2\nFAILED tests/test_cart.py::test_add - assert 1 == 2\n===== 1 failed, 40 passed, 3 skipped in 1.23s =====\n`)!;
  assert.deepEqual([py.runner, py.passed, py.failed, py.skipped], ["pytest", 40, 1, 3]);
  assert.deepEqual(py.failures, ["tests/test_cart.py::test_add"]);
  assert.equal(py.firstError, "assert 1 == 2");
});

test("go counts tests with -v and packages without", () => {
  const v = testRunOf(`=== RUN   TestAdd\n--- PASS: TestAdd (0.00s)\n=== RUN   TestSub\n    cart_test.go:12: got 1, want 2\n--- FAIL: TestSub (0.00s)\nFAIL\nFAIL\texample.com/cart\t0.01s\n`)!;
  assert.deepEqual([v.unit, v.passed, v.failed], ["test", 1, 1]);
  assert.equal(v.firstError, "cart_test.go:12: got 1, want 2");
  const quiet = testRunOf(`ok  \texample.com/a\t0.01s\nok  \texample.com/b\t(cached)\n`)!;
  assert.deepEqual([quiet.unit, quiet.passed, quiet.failed], ["package", 2, 0]);
  assert.equal(testHeadline(quiet), "All 2 packages passed");
});

test("colours are ignored and other output is not a test run", () => {
  assert.equal(testHeadline(testRunOf("\x1b[32m# pass 3\x1b[0m\n# fail 0\n")!), "All 3 tests passed");
  assert.equal(testRunOf("Compiling parzi-core\nFinished dev profile"), null);
  assert.equal(testRunOf("all tests passed, I think"), null);
  assert.equal(testRunOf(""), null);
});
