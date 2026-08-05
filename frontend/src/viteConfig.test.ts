import { describe, expect, it } from "vitest";

import viteConfig from "../vite.config";

async function configForMode(mode: string) {
  if (typeof viteConfig !== "function") {
    throw new Error("Expected Vite config factory");
  }
  return viteConfig({
    command: "serve",
    mode,
    isSsrBuild: false,
    isPreview: false,
  });
}

describe("Vite optimized dependency cache", () => {
  it("isolates native and web-mode servers under the dev-fresh cache root", async () => {
    const nativeConfig = await configForMode("development");
    const webConfig = await configForMode("web");

    expect(nativeConfig.cacheDir).toMatch(/node_modules\/\.vite\/native$/);
    expect(webConfig.cacheDir).toMatch(/node_modules\/\.vite\/web$/);
    expect(nativeConfig.cacheDir).not.toBe(webConfig.cacheDir);
  });
});
