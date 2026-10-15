// @author ixxxxoooo
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));

// 多页面构建:主工作台 / 闪念胶囊 / 轻量提示
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  build: {
    target: "safari15",
    chunkSizeWarningLimit: 2500,
    rollupOptions: {
      input: {
        main: resolve(root, "index.html"),
        capsule: resolve(root, "capsule.html"),
        toast: resolve(root, "toast.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["src/test-setup.ts"],
  },
});
