/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test-setup.ts"],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // The Rust build writes into src-tauri/target while this watcher runs,
      // and a freshly linked binary is still locked for a moment: chokidar
      // then fails with EBUSY, the dev server exits, and `tauri dev` takes the
      // app down with it. The Tauri CLI watches src-tauri itself, so nothing
      // is lost by keeping Vite out of it.
      ignored: ["**/src-tauri/**"],
    }
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) {
            return undefined;
          }
          if (id.includes("/react/") || id.includes("/react-dom/")) {
            return "vendor-react";
          }
          if (id.includes("/lucide-react/") || id.includes("/lucide-react/dist/")) {
            return "vendor-icons";
          }
          if (id.includes("/@xterm/")) {
            return "vendor-terminal";
          }
          if (id.includes("/@tauri-apps/")) {
            return "vendor-tauri";
          }
          if (
            id.includes("/recharts/") ||
            id.includes("/d3-") ||
            id.includes("/victory-vendor/") ||
            id.includes("/react-smooth/") ||
            id.includes("/react-transition-group/") ||
            id.includes("/decimal.js-light/")
          ) {
            return "vendor-charts";
          }
          if (id.includes("/@tanstack/")) {
            return "vendor-table";
          }
          if (id.includes("/monaco-editor/") || id.includes("/@monaco-editor/")) {
            return "vendor-monaco";
          }
          if (
            id.includes("/class-variance-authority/") ||
            id.includes("/clsx/") ||
            id.includes("/tailwind-merge/")
          ) {
            return "vendor-ui";
          }
          return "vendor";
        }
      }
    }
  }
});
