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

/**
 * Web mode swaps the whole `api` object for `mockApi` (src/lib/tauri.ts), so
 * every `api.projects.*` method App.tsx hands to a component must exist here.
 * The invoke-level mocks in src/mocks/tauri-api-core.ts do NOT cover this path.
 */
describe("mockProjectsApi project-creation probes", () => {
  beforeEach(() => {
    resetStore();
  });

  it("exposes every api.projects method the project creation wizard consumes", () => {
    expect(typeof mockProjectsApi.inspectProjectCandidate).toBe("function");
    expect(typeof mockProjectsApi.prepareNewProjectDirectory).toBe("function");
    expect(typeof mockProjectsApi.discardPreparedProjectDirectory).toBe("function");
  });

  it("returns a camelCase repository verdict, not the snake_case wire shape", async () => {
    await expect(
      mockProjectsApi.inspectProjectCandidate("/Users/test/projects/test-project"),
    ).resolves.toEqual({
      kind: "repository",
      repositoryRoot: "/Users/test/projects/test-project",
      currentBranch: "main",
      defaultBranch: "main",
      branches: ["main", "develop"],
      hasCommits: true,
      isDirty: false,
      capability: { kind: "localOnly" },
      alreadyRegisteredAs: null,
    });
  });

  it("reports an already-registered folder so the wizard can block it", async () => {
    const created = await mockProjectsApi.create({
      name: "Existing",
      workingDirectory: "/Users/test/projects/already-added",
    });

    await expect(
      mockProjectsApi.inspectProjectCandidate("/Users/test/projects/already-added"),
    ).resolves.toMatchObject({
      kind: "repository",
      alreadyRegisteredAs: { id: created.id, name: "Existing" },
    });
  });

  it("returns notFound for an empty path", async () => {
    await expect(mockProjectsApi.inspectProjectCandidate("")).resolves.toEqual({
      kind: "notFound",
    });
  });

  it("composes the prepared destination path", async () => {
    await expect(
      mockProjectsApi.prepareNewProjectDirectory({
        parentDirectory: "/Users/test/projects",
        folderName: "my-app",
      }),
    ).resolves.toEqual({ path: "/Users/test/projects/my-app", created: true });

    await expect(
      mockProjectsApi.discardPreparedProjectDirectory("/Users/test/projects/my-app"),
    ).resolves.toBeUndefined();
  });
});
