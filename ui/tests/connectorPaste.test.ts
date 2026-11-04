import { test } from "node:test";
import assert from "node:assert/strict";
import { remoteFromPaste, withBearer } from "../src/lib/connectorCatalog.ts";

test("a pasted link keeps a bearer token", () => {
  const pasted = remoteFromPaste("https://os.example.com/mcp\nAuthorization: Bearer secret\n");
  assert.equal(pasted?.url, "https://os.example.com/mcp");
  assert.equal(pasted?.headers?.Authorization, "Bearer secret");
  assert.equal(remoteFromPaste("https://os.example.com/mcp\nsecret")?.headers?.Authorization, "Bearer secret");
  assert.equal(remoteFromPaste("not a link"), null);
});

test("a typed token is attached, and a blank one leaves a saved header", () => {
  const link = remoteFromPaste("https://os.example.com/mcp");
  assert.ok(link);
  assert.equal(withBearer(link, "secret").headers?.Authorization, "Bearer secret");
  assert.equal(withBearer(link, "Bearer secret").headers?.Authorization, "Bearer secret");
  const kept = remoteFromPaste("https://os.example.com/mcp\nAuthorization: Basic abc");
  assert.equal(kept?.headers?.Authorization, "Basic abc");
  assert.equal(withBearer(kept!, "").headers?.Authorization, "Basic abc");
  assert.equal(withBearer({ command: "npx" }, "secret").headers, undefined);
});
