// Dev server for `tauri dev` (beforeDevCommand): Tauri windows load `index.html` / `overlay.html`, the paths of the
// built `dist/`, while bun's `bun ./index.html` CLI would serve them as `/` and `/overlay`.
import index from "../index.html";
import overlay from "../overlay.html";

Bun.serve({
  port: 1420,
  // bun 1.4.3 HMR leaves CSS module imports undefined (`import_App_module` ReferenceError).
  development: { hmr: false, console: true },
  routes: { "/": index, "/index.html": index, "/overlay.html": overlay },
});
