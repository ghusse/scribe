// Production build of the two Tauri windows into `dist/` (tauri.conf.json > frontendDist). The folder is emptied
// first: bun does not clean it and Tauri embeds everything it contains.
import { rmSync } from "node:fs";

rmSync("dist", { recursive: true, force: true });
const result = await Bun.build({
  entrypoints: ["index.html", "overlay.html"],
  outdir: "dist",
  minify: true,
  splitting: true,
});
for (const log of result.logs) console.error(log);
if (!result.success) process.exit(1);
for (const output of result.outputs) console.log(output.path);
