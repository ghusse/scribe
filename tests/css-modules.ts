// CSS modules compiled by bun's own bundler, as in the production build: real (hashed) class names, and a class the
// stylesheet does not define stays undefined. bun test alone would import every `.module.css` as `{}`.
import { plugin } from "bun";

const ENTRY = "css-module-entry";

async function compile(path: string): Promise<string> {
  const result = await Bun.build({
    entrypoints: [ENTRY],
    target: "bun",
    plugins: [
      {
        name: "css-module-entry",
        setup(build) {
          build.onResolve({ filter: new RegExp(`^${ENTRY}$`) }, () => ({ path: ENTRY, namespace: ENTRY }));
          build.onLoad({ filter: /.*/, namespace: ENTRY }, () => ({
            contents: `export { default } from ${JSON.stringify(path)};`,
            loader: "js",
          }));
        },
      },
    ],
  });
  const js = result.outputs.find((o) => o.kind === "entry-point");
  if (!result.success || !js) throw new AggregateError(result.logs, `cannot compile ${path}`);
  return js.text();
}

plugin({
  name: "css-modules",
  setup(build) {
    build.onLoad({ filter: /\.module\.css$/ }, async ({ path }) => ({ contents: await compile(path), loader: "js" }));
  },
});
