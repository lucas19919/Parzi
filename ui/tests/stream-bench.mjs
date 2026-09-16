/** Entry point: register the loader, then run the benchmark. */
import { register } from "node:module";
register("./stream-loader.mjs", import.meta.url);
await import("./stream-bench.ts");
