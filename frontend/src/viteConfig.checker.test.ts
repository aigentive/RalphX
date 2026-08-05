import { describe, expect, it, vi } from "vitest";

const checkerMock = vi.hoisted(() => vi.fn(() => ({ name: "test-checker" })));

vi.mock("vite-plugin-checker", () => ({ default: checkerMock }));

import viteConfig from "../vite.config";

describe("Vite TypeScript checker", () => {
  it("pins project resolution to the active frontend checkout", async () => {
    if (typeof viteConfig !== "function") {
      throw new Error("Expected Vite config factory");
    }
    await viteConfig({
      command: "serve",
      mode: "development",
      isSsrBuild: false,
      isPreview: false,
    });

    expect(checkerMock).toHaveBeenCalledWith(expect.objectContaining({
      typescript: {
        root: expect.stringMatching(/frontend$/),
        tsconfigPath: "tsconfig.json",
      },
    }));
  });
});
