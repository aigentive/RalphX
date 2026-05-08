import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: true,
    environment: "node",
    include: ["tests/**/*.test.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "json-summary", "html", "lcov"],
      exclude: ["build/**", "node_modules/**", "**/*.d.ts", "vitest.config.ts"],
    },
  },
});
