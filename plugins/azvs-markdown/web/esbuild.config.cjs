const esbuild = require("esbuild");
const fs = require("node:fs");
const path = require("node:path");

const root = __dirname;
const distRoot = path.join(root, "dist");

fs.rmSync(distRoot, { recursive: true, force: true });
fs.mkdirSync(distRoot, { recursive: true });

esbuild.buildSync({
  entryPoints: ["src/viewer.ts"],
  outfile: "dist/viewer.js",
  bundle: true,
  format: "iife",
  target: ["es2020"],
  platform: "browser",
  sourcemap: true,
});

fs.copyFileSync(path.join(root, "src/index.html"), path.join(distRoot, "index.html"));
fs.copyFileSync(path.join(root, "src/viewer.css"), path.join(distRoot, "viewer.css"));
