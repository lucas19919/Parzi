import { test } from "node:test";
import assert from "node:assert/strict";
import { next, parse } from "./release.mjs";

test("a dev build counts up, and starts after a full release", () => {
  assert.equal(next("dev", "1.4.1-dev.2"), "1.4.1-dev.3");
  assert.equal(next("dev", "1.4.0"), "1.4.1-dev.1");
  assert.throws(() => next("dev", "1.4.1-rc.1"), /next dev build/);
});

test("a full release takes the dev builds' version, or counts up", () => {
  assert.equal(next("patch", "1.4.1-dev.3"), "1.4.1");
  assert.equal(next("patch", "1.4.1"), "1.4.2");
  assert.equal(next("minor", "1.5.0-dev.2"), "1.5.0");
  assert.equal(next("minor", "1.4.1-dev.2"), "1.5.0");
  assert.equal(next("major", "1.4.1"), "2.0.0");
  assert.equal(next("2.0.0-dev.1", "1.4.1"), "2.0.0-dev.1");
});

test("versions are checked", () => {
  assert.deepEqual(parse("v1.2.3-dev.4"), { major: 1, minor: 2, patch: 3, pre: "dev.4" });
  assert.throws(() => parse("1.2"), /not a version/);
  assert.throws(() => next("soon", "1.2.3"), /not a version/);
});
