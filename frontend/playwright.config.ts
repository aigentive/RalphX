import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for end-to-end testing.
 * Runs against the web mode dev server (npm run dev:web).
 * Includes both visual regression tests and integration tests.
 *
 * @see https://playwright.dev/docs/test-configuration
 */
export default defineConfig({
  testDir: "./tests",
  /* Run tests in files in parallel */
  fullyParallel: true,
  /* Fail the build on CI if you accidentally left test.only in the source code. */
  forbidOnly: !!process.env.CI,
  /* Retry on CI only */
  retries: process.env.CI ? 2 : 0,
  /* Opt out of parallel tests on CI. */
  workers: process.env.CI ? 1 : undefined,
  /* Reporter to use. See https://playwright.dev/docs/test-reporters */
  reporter: "html",
  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    /* Base URL to use in actions like `await page.goto('/')`. */
    // Use 127.0.0.1 (not localhost) to match the IPv4 binding `vite.config.ts`
    // pins for the dev server. Otherwise macOS resolves `localhost` to `::1`
    // first, the connection refuses (Vite is IPv4-only), and the IPv6→IPv4
    // fallback adds enough latency to delay font/animation settle and shift
    // the rendered output by ~1 px — enough to push visual snapshot diffs
    // past their `maxDiffPixelRatio` threshold on macos-15 CI runners.
    baseURL: "http://127.0.0.1:5173",
    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: "on-first-retry",
    /* Keep an end-of-test screenshot in the HTML report for every visual run. */
    screenshot: "on",
  },

  /* Configure projects for major browsers */
  projects: [
    {
      name: "chromium",
      testIgnore: /docs-capture/,
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "docs-capture",
      testDir: "./tests/docs-capture",
      testMatch: "**/*.spec.ts",
      snapshotDir: "./tests/docs-capture/snapshots",
      use: {
        ...devices["Desktop Chrome"],
        // 1728x1080 is the MacBook Pro 16" default scaled resolution, and it is
        // the smallest realistic window that fits the whole Agents split layout:
        // 72px rail + 340px session list + AGENTS_CHAT_MIN_WIDTH (600) +
        // AGENTS_ARTIFACT_MIN_WIDTH (600) = 1612px. Anything narrower clips the
        // right artifact pane, which is the subject of most guide screenshots.
        viewport: { width: 1728, height: 1080 },
        deviceScaleFactor: 2,
        scale: "device",
      },
      expect: {
        toHaveScreenshot: {
          // Docs-capture screenshots illustrate documentation; they tolerate
          // cross-environment rendering variance (font hinting, subpixel AA)
          // that the stricter visual-regression suite does not.
          maxDiffPixelRatio: 0.03,
        },
      },
    },
  ],

  /* Run your local dev server before starting the tests */
  webServer: {
    command: "npm run dev:web",
    // Match `baseURL` above — IPv4-only readiness probe avoids the same
    // ::1-then-127.0.0.1 fallback latency Playwright would otherwise hit.
    url: "http://127.0.0.1:5173",
    reuseExistingServer: !process.env.CI,
    timeout: 120 * 1000,
    // Give Vite a chance to close sockets before reaping. Without this Playwright
    // can default to SIGKILL on teardown (worse on macOS), which orphans hundreds
    // of TIME_WAITs on ::1:5173 every run. With many parallel agent worktrees
    // running tests, those leaks accumulate and exhaust the ephemeral port pool.
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
  },

  /* Directory for snapshot files */
  snapshotDir: "./tests/visual/snapshots",

  /* Update snapshots on CI */
  expect: {
    toHaveScreenshot: {
      /* Threshold for pixel difference */
      maxDiffPixelRatio: 0.01,
    },
  },
});
