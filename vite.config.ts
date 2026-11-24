/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    environment: "jsdom",
    // vmThreads, not the default threads pool: with the default worker_threads
    // pool the setup file's afterEach intermittently runs before the runner
    // establishes the file suite ("Vitest failed to find the current suite"),
    // failing every file — flaky per invocation and worse on node >= 26.
    // The vm-isolated pool keeps the setup hooks in the same VM context and
    // runs green deterministically on node 24 and 26.
    pool: "vmThreads",
    // Recharts's CJS build requires @reduxjs/toolkit, whose package exports
    // expose "module-sync" -> .mjs. On Node 20.19+ under vmThreads, Vitest's
    // CJS VM executor resolves "module-sync" to the ESM file and fails with
    // SyntaxError: Unexpected token 'export'. Inlining recharts (and its
    // ESM-in-CJS deps) routes them through Vite's ESM transform on Node 20+.
    server: {
      deps: {
        inline: ["recharts", "@reduxjs/toolkit", "victory-vendor"],
      },
    },
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
    // No manualChunks here, deliberately. A previous vendor-split config
    // (vendor-react / vendor-monaco / vendor / …) produced a chunk cycle:
    // vendor-react imported the monaco loader chunk, which imported the
    // generic vendor chunk, whose CJS packages eagerly require("react") at
    // module top level. Function-declaration hoisting let react's require run
    // before react's module body initialized its exports object, crashing at
    // boot with "Cannot set properties of undefined (setting 'Children')" —
    // a fully blank window in the packaged app (0.4.5 Windows install).
    // Any react-out-of-tree split is unsafe while eager CJS requires exist,
    // so node_modules stay in the entry chunk; the monaco/FilePreview chain
    // remains a separate chunk on its own because it is lazy-imported.
    rollupOptions: {}
  }
});
