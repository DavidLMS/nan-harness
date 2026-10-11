import { readFile, readdir, mkdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const sourceRoot = process.env.ZCODE_SOURCE_ROOT;
if (!sourceRoot || !process.argv[2]) {
  throw new Error("Set ZCODE_SOURCE_ROOT and provide a temporary output directory.");
}
const output = resolve(process.argv[2]);
const { build } = await import(
  pathToFileURL(join(sourceRoot, "node_modules/esbuild/lib/main.js")).href
);
const packages = new Map();
for (const directory of ["packages", "apps/zcode-cli/packages", "apps/zcode-cli/tools"]) {
  for (const name of await readdir(join(sourceRoot, directory))) {
    const base = join(sourceRoot, directory, name);
    let text;
    try {
      text = await readFile(join(base, "package.json"), "utf8");
    } catch (error) {
      if (error.code === "ENOENT" || error.code === "ENOTDIR") continue;
      throw error;
    }
    const manifest = JSON.parse(text);
    packages.set(manifest.name, { base, manifest });
  }
}

const sourcePlugin = {
  name: "official-zcode-source",
  setup(builder) {
    builder.onResolve({ filter: /^zcode-source\// }, ({ path }) => ({
      path: resolve(sourceRoot, path.slice("zcode-source/".length)),
    }));
    builder.onResolve({ filter: /^@zcode\// }, ({ path }) => {
      const parts = path.split("/");
      const entry = packages.get(parts.slice(0, 2).join("/"));
      if (!entry) throw new Error(`Unknown upstream package: ${path}`);
      const subpath = parts.length === 2 ? "." : `./${parts.slice(2).join("/")}`;
      let target = entry.manifest.exports?.[subpath];
      if (target === undefined && subpath === ".") target = entry.manifest.main;
      if (typeof target === "object") target = target.import ?? target.default;
      target ??= subpath === "." ? "./src/index.ts" : `${subpath}.ts`;
      return {
        path: resolve(entry.base, target.replace("/dist/", "/src/").replace(/\.js$/, ".ts")),
      };
    });
  },
};

await mkdir(output, { recursive: true });
for (const probe of ["config-probe", "protocol-probe"]) {
  await build({
    entryPoints: [fileURLToPath(new URL(`./${probe}.mjs`, import.meta.url))],
    bundle: true,
    platform: "node",
    format: "esm",
    outfile: join(output, `${probe}.mjs`),
    nodePaths: [join(sourceRoot, "node_modules")],
    banner: {
      js: "import {createRequire} from 'node:module'; const require=createRequire(import.meta.url);",
    },
    plugins: [sourcePlugin],
  });
}
