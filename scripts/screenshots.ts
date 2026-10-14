// Captures the README screenshots (docs/screenshots/*.png) from the real UI running on fictional data
// (scripts/screenshots/tauri-demo.ts), in headless Chrome. Chrome path: $CHROME, else the usual install paths.
import { existsSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import main from "./screenshots/main.html";
import overlay from "./screenshots/overlay.html";
import scene from "./screenshots/scene.html";

const SHOTS: { file: string; path: string; size: [number, number] }[] = [
  { file: "dictation.png", path: "/scene.html", size: [980, 560] },
  { file: "history.png", path: "/main.html?tab=Historique", size: [980, 700] },
  { file: "glossary.png", path: "/main.html?tab=Vocabulaire", size: [980, 440] },
  { file: "settings.png", path: "/main.html?tab=Réglages", size: [980, 700] },
  { file: "setup.png", path: "/main.html?tab=Réglages&section=Fournisseurs et modèles", size: [980, 700] },
];

const CHROMES = [
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
];
const chrome = process.env.CHROME ?? CHROMES.find((p) => existsSync(p));
if (!chrome) throw new Error("Chrome not found: set CHROME to its executable");

const server = Bun.serve({
  port: 0,
  development: { hmr: false },
  routes: { "/main.html": main, "/overlay.html": overlay, "/scene.html": scene },
});

const out = resolve("docs/screenshots");
mkdirSync(out, { recursive: true });
try {
  for (const shot of SHOTS) {
    const file = resolve(out, shot.file);
    const proc = Bun.spawn(
      [
        chrome,
        "--headless",
        "--hide-scrollbars",
        "--force-device-scale-factor=2",
        "--lang=fr-FR",
        "--blink-settings=preferredColorScheme=1",
        "--virtual-time-budget=3000",
        `--window-size=${shot.size.join(",")}`,
        `--screenshot=${file}`,
        new URL(shot.path, server.url).href,
      ],
      { stdout: "ignore", stderr: "pipe" },
    );
    if ((await proc.exited) !== 0) throw new Error(`${shot.file}: ${await new Response(proc.stderr).text()}`);
    console.log(file);
  }
} finally {
  server.stop(true);
}
