import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['src/__tests__/**/*.test.ts'],
    exclude: ['build/**', 'node_modules/**', '.cache/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'json-summary', 'html', 'lcov'],
      exclude: ['build/**', 'node_modules/**', '.cache/**', '**/*.d.ts', 'vitest.config.ts'],
    },
  },
});
