import { test } from "node:test";
import assert from "node:assert/strict";
import { mathAt } from "../src/lib/mdMath.ts";

// Every math span in a line, as the renderer would find them.
function spans(line: string): string[] {
  const out: string[] = [];
  for (let i = 0; i < line.length; i++) {
    const m = mathAt(line, i);
    if (m) {
      out.push(m.tex);
      i = m.next - 1;
    }
  }
  return out;
}

test("money is text, even two amounts in a sentence", () => {
  assert.deepEqual(spans("Visa's net revenue rose 11% to $40.0 billion, or $24.0 billion before tax."), []);
  assert.deepEqual(spans("The litigation provision was $2.6 billion against $0.5 billion a year earlier."), []);
  assert.deepEqual(spans("It costs $5, $10 or $20."), []);
  assert.deepEqual(spans("Prices run $5-$10."), []);
});

test("math is math", () => {
  assert.deepEqual(spans("The area is $\\pi r^2$ here."), ["\\pi r^2"]);
  assert.deepEqual(spans("Both $a+b$ and $c$."), ["a+b", "c"]);
  assert.deepEqual(spans("Display $$E = mc^2$$ inline."), ["E = mc^2"]);
  assert.deepEqual(spans("Grows like $n^2$, about $10^6$ steps."), ["n^2", "10^6"]);
});

test("escaped and spaced dollars are not math", () => {
  assert.deepEqual(spans("A \\$5 fee and a \\$10 fee."), []);
  assert.deepEqual(spans("From $ x $ to y."), []);
});
