import { defineConfig } from "vite";

export default defineConfig({
  build: {
    emptyOutDir: true,
    lib: {
      entry: "src/index.ts",
      name: "AssetHubPlugin",
      formats: ["es", "iife"],
      fileName: (format) =>
        format === "es" ? "asset-hub-plugin.js" : "asset-hub-plugin.global.js",
    },
    sourcemap: true,
  },
});
