import { beforeEach, describe, expect, it } from "vitest";

import { mockProjectsApi } from "./projects";
import { resetStore } from "./store";

describe("mockProjectsApi PR templates", () => {
  beforeEach(() => {
    resetStore();
  });

  it("returns null until a project template is written", async () => {
    await expect(
      mockProjectsApi.readPrTemplate("project-without-template"),
    ).resolves.toBeNull();
  });

  it("stores exact PR template content per project", async () => {
    await mockProjectsApi.writePrTemplate("project-template-a", "## Summary\n");
    await mockProjectsApi.writePrTemplate("project-template-b", "");

    await expect(
      mockProjectsApi.readPrTemplate("project-template-a"),
    ).resolves.toBe("## Summary\n");
    await expect(
      mockProjectsApi.readPrTemplate("project-template-b"),
    ).resolves.toBe("");
  });
});
