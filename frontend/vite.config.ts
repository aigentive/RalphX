import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import checker from "vite-plugin-checker";
import path from "path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => {
  // Web mode: use mock implementations for Tauri plugins
  const isWebMode = mode === "web";

  // Base alias always includes the @ -> src mapping
  const baseAlias: Record<string, string> = {
    "@": path.resolve(__dirname, "./src"),
    // Force dagre packages to use CJS builds (ESM build has broken require() calls)
    "@dagrejs/dagre": path.resolve(
      __dirname,
      "./node_modules/@dagrejs/dagre/dist/dagre.cjs.js"
    ),
    "@dagrejs/graphlib": path.resolve(
      __dirname,
      "./node_modules/@dagrejs/graphlib/dist/graphlib.cjs.js"
    ),
  };

  // In web mode, alias Tauri APIs and plugins to our mock implementations
  const webModeAliases: Record<string, string> = isWebMode
    ? {
        // Core Tauri APIs
        "@tauri-apps/api/core": path.resolve(
          __dirname,
          "./src/mocks/tauri-api-core.ts"
        ),
        "@tauri-apps/api/event": path.resolve(
          __dirname,
          "./src/mocks/tauri-api-event.ts"
        ),
        "@tauri-apps/api/webview": path.resolve(
          __dirname,
          "./src/mocks/tauri-api-webview.ts"
        ),
        // Tauri plugins
        "@tauri-apps/plugin-dialog": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-dialog.ts"
        ),
        "@tauri-apps/plugin-fs": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-fs.ts"
        ),
        "@tauri-apps/plugin-process": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-process.ts"
        ),
        "@tauri-apps/plugin-updater": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-updater.ts"
        ),
        "@tauri-apps/plugin-global-shortcut": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-global-shortcut.ts"
        ),
      }
    : {};

  return {
    define: {
      // UI debug logging is opt-in via shell env, e.g. DEBUG=true npm run tauri dev
      __UI_DEBUG__: JSON.stringify(process.env.DEBUG === "true"),
    },
    plugins: [
      react(),
      tailwindcss(),
      checker({
        typescript: true,
        overlay: {
          initialIsOpen: false,
          position: "br",
        },
      }),
    ],
    resolve: {
      alias: {
        ...baseAlias,
        ...webModeAliases,
      },
    },

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent Vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    //    web mode uses port 5173 to avoid conflict with native dev server
    server: {
      port: isWebMode ? 5173 : 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: isWebMode ? 5174 : 1421,
          }
        : undefined,
      watch: {
        // 3. tell Vite to ignore backend trees, generated artifacts, and test outputs
        // that can churn heavily during local validation without affecting app source.
        ignored: [
          "**/src-tauri/**",
          "**/.artifacts/**",
          "**/logs/**",
          "**/*.md",
          "**/test-results/**",
          "**/playwright-report/**",
          "**/tests/visual/snapshots/**",
        ],
      },
    },

    // Handle mixed ESM/CJS dependencies (dagre uses require() in its ESM build)
    optimizeDeps: {
      include: ["@dagrejs/dagre", "@dagrejs/graphlib"],
      rolldownOptions: {
        output: {
          // Suppress dependency sourcemap generation in dev pre-bundles.
          // This avoids noisy *.js.map access-control warnings in the Tauri webview.
          sourcemap: false,
        },
        resolve: {
          // Force CommonJS for dagre packages
          mainFields: ["main", "module"],
        },
      },
    },
    build: {
      rolldownOptions: {
        output: {
          // Preserve production chunk execution order so dependency chunks do not
          // call app-entry re-exports before the entry module initializes them.
          strictExecutionOrder: true,
        },
      },
      commonjsOptions: {
        transformMixedEsModules: true,
        include: [/node_modules/],
      },
    },
  };
});
