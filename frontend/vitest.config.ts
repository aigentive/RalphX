/// <reference types="vitest" />
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "path";

// Node 24+ enables the WebStorage API (localStorage/sessionStorage) by default.
// Its built-in localStorage emits "`--localstorage-file` was provided without a
// valid path" on any access (and has a broken setItem), so we disable it in the
// test workers and let jsdom own localStorage. Guard on the flag's existence so
// Node <=22 (e.g. CI) — where the flag is unknown — does not crash with
// "bad option".
const disableNodeWebStorage =
  process.allowedNodeEnvironmentFlags.has("--experimental-webstorage")
    ? ["--no-experimental-webstorage"]
    : [];

export default defineConfig({
  define: {
    __UI_DEBUG__: JSON.stringify(false),
  },
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@tauri-apps/plugin-notification": path.resolve(
        __dirname,
        "./src/mocks/tauri-plugin-notification.ts"
      ),
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
    pool: "forks",
    execArgv: disableNodeWebStorage,
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "json-summary", "html", "lcov"],
      exclude: [
        "node_modules/",
        "src/test/",
        "**/*.d.ts",
        "**/*.config.*",
        "**/types/**",
      ],
    },
  },
});
