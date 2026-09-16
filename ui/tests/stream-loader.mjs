/** Resolve hook: point `dompurify` at the node stub. Registered by
    `stream-bench.mjs`; used only by the benchmark, never by the app. */
const STUB = new URL("./stream-dompurify.mjs", import.meta.url).href;
export async function resolve(spec, ctx, next) {
  if (spec === "dompurify") return { url: STUB, shortCircuit: true };
  return next(spec, ctx);
}
