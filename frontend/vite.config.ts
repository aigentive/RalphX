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
        "@tauri-apps/plugin-notification": path.resolve(
          __dirname,
          "./src/mocks/tauri-plugin-notification.ts"
        ),
      }
    : {};

  return {
    // Native Tauri dev and web-mode Playwright can run concurrently from the
    // same checkout. Separate their pre-bundles so either server can rebuild or
    // shut down without invalidating the other's optimized dependency hashes.
    // dev-fresh already clears the shared parent directory before launch.
    cacheDir: path.resolve(
      __dirname,
      "./node_modules/.vite",
      isWebMode ? "web" : "native",
    ),
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
      // Bind to IPv4 explicitly. With `host: false` (the previous default) Vite 8 on
      // macOS / Node 22 resolves `localhost` to `::1` (IPv6) and binds there. If a prior
      // dev session crashed leaving TIME_WAIT sockets on `::1:1420`, the next Vite start
      // fails with EADDRINUSE even though `lsof` reports nothing listening. Pinning to
      // 127.0.0.1 sidesteps that — IPv6 TIME_WAITs are irrelevant when we bind IPv4 only.
      // Mobile/Tauri DEV_HOST mode (when `host` is set) still wins.
      //
      // Hard incident (2026-05-18): a chain of Playwright runs from agent worktrees
      // that branched off main before the IPv4 pin landed left ~10,800 orphaned
      // TIME_WAITs on ::1:5173. They never drained, exhausted the ephemeral port
      // pool (49152-65535), and Chrome started failing with ERR_ADDRESS_INVALID
      // because bind() couldn't allocate a source port. Pin HMR explicitly too so
      // the contract is unambiguous on every code path.
      host: host || "127.0.0.1",
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: isWebMode ? 5174 : 1421,
          }
        : {
            protocol: "ws",
            host: "127.0.0.1",
            port: isWebMode ? 5174 : 1421,
          },
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
