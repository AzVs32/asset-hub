import { defineConfig } from "vite";

export default defineConfig({
  build: {
    emptyOutDir: false,
    lib: {
      entry: "src/contract.ts",
      formats: ["es"],
      fileName: () => "contract.js",
    },
    sourcemap: false,
  },
});
