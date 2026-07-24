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

  it("creates fresh local-only projects and preserves their capability across updates", async () => {
    const created = await mockProjectsApi.create({
      name: "Repository parity",
      workingDirectory: "/tmp/repository-parity",
      gitMode: "worktree",
      baseBranch: "develop",
      worktreeParentDirectory: "/tmp/worktrees",
    });

    expect(created).toMatchObject({
      baseBranch: "develop",
      worktreeParentDirectory: "/tmp/worktrees",
      githubPrEnabled: false,
      repositoryCapability: { kind: "localOnly" },
    });
    expect(await mockProjectsApi.get(created.id)).toEqual(created);

    await expect(
      mockProjectsApi.update(created.id, { baseBranch: "release" }),
    ).resolves.toMatchObject({
      baseBranch: "release",
      worktreeParentDirectory: "/tmp/worktrees",
      githubPrEnabled: false,
      repositoryCapability: { kind: "localOnly" },
    });
    await expect(mockProjectsApi.get(created.id)).resolves.toMatchObject({
      baseBranch: "release",
      repositoryCapability: { kind: "localOnly" },
    });
  });
});
