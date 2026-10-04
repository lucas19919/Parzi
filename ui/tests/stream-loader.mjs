const STUB = new URL("./stream-dompurify.mjs", import.meta.url).href;
export async function resolve(spec, ctx, next) {
  if (spec === "dompurify") return { url: STUB, shortCircuit: true };
  return next(spec, ctx);
}
