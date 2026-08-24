import { defineConfig } from "vite";

export default defineConfig({
  build: {
    emptyOutDir: true,
    lib: {
      entry: "src/index.ts",
      name: "AssetWebSdk",
      formats: ["es", "iife"],
      fileName: (format) => (format === "es" ? "asset-web-sdk.js" : "asset-web-sdk.global.js"),
    },
    sourcemap: false,
  },
});
