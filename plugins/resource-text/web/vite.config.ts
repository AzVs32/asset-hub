import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  base: "./",
  plugins: [react()],
  build: {
    outDir: "../asset-plugin-target",
    // build.sh owns target cleanup so the Web build cannot remove plugin.wasm.
    emptyOutDir: false,
    sourcemap: false,
  },
});
