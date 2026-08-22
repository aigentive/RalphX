import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { mockGuideWorkspaceFileDiffPage } from "./guide-diff-pages";
import {
  GUIDE_SCENARIO_FIXTURES,
  PROD_UI_FEATURE_FLAGS,
  seedGuideStore,
} from "./guide-scenarios";
import { getStore } from "./store";

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

  it("detects language from file extension", () => {
    expect(mockGuideWorkspaceFileDiffPage("lib.rs", 0, 1).language).toBe("rust");
    expect(mockGuideWorkspaceFileDiffPage("config.yaml", 0, 1).language).toBe("yaml");
    expect(mockGuideWorkspaceFileDiffPage("config.yml", 0, 1).language).toBe("yaml");
    expect(mockGuideWorkspaceFileDiffPage("README.md", 0, 1).language).toBe("text");
  });

  it("paginates rows with next_offset", () => {
    const page = mockGuideWorkspaceFileDiffPage("file.tsx", 0, 2);
    expect(page.rows).toHaveLength(2);
    expect(page.next_offset).toBe(2);
  });
});

describe("guide scenario fixtures", () => {
  it("provides all named scenarios", () => {
    const names = Object.keys(GUIDE_SCENARIO_FIXTURES);
    expect(names).toContain("guide_onboarding");
    expect(names).toContain("guide_implementing");
    expect(names).toHaveLength(7);
  });

  it("guide_implementing includes tool-use content blocks", () => {
    const impl = GUIDE_SCENARIO_FIXTURES.guide_implementing;
    const msgs = impl.messages[impl.conversations[0]!.id]!;
    const activity = msgs.find((m) => m.id.endsWith("-activity"));
    expect(activity?.contentBlocks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: "tool_use", name: "functions.exec_command" }),
      ]),
    );
  });

  it("seedGuideStore populates mock store with scenario data", () => {
    seedGuideStore("guide_tour");
    const store = getStore();
    expect(store.projects.size).toBeGreaterThan(0);
    expect(store.tasks.size).toBeGreaterThan(0);
  });
});
