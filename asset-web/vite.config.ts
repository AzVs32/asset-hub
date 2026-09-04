import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "src") },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true,
        rewrite: (value) => value.replace(/^\/api/, ""),
      },
    },
  },
  test: {
    environment: "jsdom",
    // 测试套件暂缺，待无插件版本稳定后重建；此时 npm test 直接通过。
    passWithNoTests: true,
  },
});
