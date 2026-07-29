import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { mockGuideWorkspaceFileDiffPage } from "./guide-diff-pages";
import { PROD_UI_FEATURE_FLAGS } from "./guide-scenarios";

describe("PROD_UI_FEATURE_FLAGS", () => {
  it("matches config/ralphx.yaml ui.feature_flags key-for-key", () => {
    const yaml = readFileSync(resolve(import.meta.dirname, "../../../config/ralphx.yaml"), "utf8");
    const block = yaml.match(/^ {2}feature_flags:\n([\s\S]*?)(?=^\S|\n\S)/m)?.[1] ?? "";
    const keys = [...block.matchAll(/^ {4}([a-z_]+):/gm)].map((match) => match[1]);
    const camel = (key: string) => key.replace(/_([a-z])/g, (_, char: string) => char.toUpperCase());
    expect(Object.keys(PROD_UI_FEATURE_FLAGS).sort()).toEqual(keys.map(camel).sort());
    for (const [key, value] of Object.entries(PROD_UI_FEATURE_FLAGS)) {
      const snake = key.replace(/[A-Z]/g, (char) => `_${char.toLowerCase()}`);
      expect(yaml).toContain(`    ${snake}: ${value}`);
    }
  });
});

describe("guide workspace diff pages", () => {
  it("provides real addition and deletion rows for paged guide diffs", () => {
    const page = mockGuideWorkspaceFileDiffPage("frontend/src/ReleaseChecklist.tsx", 0, 200);

    expect(page.rows).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          kind: "line",
          line: expect.objectContaining({ kind: "addition" }),
        }),
        expect.objectContaining({
          kind: "line",
          line: expect.objectContaining({ kind: "deletion" }),
        }),
      ]),
    );
    expect(page.next_offset).toBeNull();
  });
});
