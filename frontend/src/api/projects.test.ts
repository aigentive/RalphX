import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { projectsApi } from "./projects";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("projectsApi candidate probe / prepare / discard", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("inspectProjectCandidate transforms a repository verdict to camelCase", async () => {
    mockInvoke.mockResolvedValue({
      kind: "repository",
      repository_root: "/Users/dev/my-app",
      current_branch: "main",
      default_branch: "main",
      branches: ["main", "develop"],
      has_commits: true,
      is_dirty: false,
      capability: { kind: "local_only" },
      already_registered_as: null,
    });

    const result = await projectsApi.inspectProjectCandidate("/Users/dev/my-app");

    expect(mockInvoke).toHaveBeenCalledWith("inspect_project_candidate", {
      path: "/Users/dev/my-app",
    });
    expect(result).toEqual({
      kind: "repository",
      repositoryRoot: "/Users/dev/my-app",
      currentBranch: "main",
      defaultBranch: "main",
      branches: ["main", "develop"],
      hasCommits: true,
      isDirty: false,
      capability: { kind: "localOnly" },
      alreadyRegisteredAs: null,
    });
  });

  it("inspectProjectCandidate passes through a non-repository verdict", async () => {
    mockInvoke.mockResolvedValue({ kind: "non_empty_non_repo", entry_count: 4 });

    const result = await projectsApi.inspectProjectCandidate("/Users/dev/junk");

    expect(result).toEqual({ kind: "nonEmptyNonRepo", entryCount: 4 });
  });

  it("prepareNewProjectDirectory wraps args under input and returns camelCase", async () => {
    mockInvoke.mockResolvedValue({ path: "/parent/my-app", created: true });

    const result = await projectsApi.prepareNewProjectDirectory({
      parentDirectory: "/parent",
      folderName: "my-app",
    });

    expect(mockInvoke).toHaveBeenCalledWith("prepare_new_project_directory", {
      input: { parentDirectory: "/parent", folderName: "my-app" },
    });
    expect(result).toEqual({ path: "/parent/my-app", created: true });
  });

  it("discardPreparedProjectDirectory invokes with the flat path", async () => {
    mockInvoke.mockResolvedValue(null);

    await projectsApi.discardPreparedProjectDirectory("/parent/my-app");

    expect(mockInvoke).toHaveBeenCalledWith("discard_prepared_project_directory", {
      path: "/parent/my-app",
    });
  });
});
