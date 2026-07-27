import { describe, it, expect } from "vitest";
import {
  GitModeSchema,
  ProjectSchema,
  CreateProjectSchema,
  getProjectWorkspacePublishMode,
  transformProject,
  UpdateProjectSchema,
} from "./project";

describe("GitModeSchema", () => {
  it("should parse 'worktree' as valid", () => {
    expect(GitModeSchema.parse("worktree")).toBe("worktree");
  });

  it("should reject invalid git modes", () => {
    expect(() => GitModeSchema.parse("remote")).toThrow();
    expect(() => GitModeSchema.parse("local")).toThrow();
    expect(() => GitModeSchema.parse("")).toThrow();
  });
});

describe("ProjectSchema", () => {
  const validProject = {
    id: "550e8400-e29b-41d4-a716-446655440000",
    name: "Test Project",
    working_directory: "/path/to/project",
    git_mode: "worktree" as const,
    base_branch: null,
    worktree_parent_directory: null,
    use_feature_branches: true,
    merge_validation_mode: "block" as const,
    detected_analysis: null,
    custom_analysis: null,
    analyzed_at: null,
    created_at: "2026-01-24T12:00:00Z",
    updated_at: "2026-01-24T12:00:00Z",
  };

  it("should parse a valid project", () => {
    expect(() => ProjectSchema.parse(validProject)).not.toThrow();
  });

  it("transforms the live repository capability from the snake_case response", () => {
    const project = transformProject(
      ProjectSchema.parse({
        ...validProject,
        repository_capability: {
          kind: "github",
          fetch_url: "https://github.com/ralphx/ralphx.git",
          push_url: "git@github.com:ralphx/ralphx.git",
        },
      })
    );

    expect(project.repositoryCapability).toEqual({
      kind: "github",
      fetchUrl: "https://github.com/ralphx/ralphx.git",
      pushUrl: "git@github.com:ralphx/ralphx.git",
    });
  });

  it("fails closed when a legacy response has no repository capability", () => {
    const project = transformProject(ProjectSchema.parse(validProject));

    expect(project.repositoryCapability).toEqual({
      kind: "inspectionFailed",
      message: expect.stringMatching(/capability is unavailable/i),
    });
  });

  it("derives workspace publishing from persisted PR authority, capability, and preference", () => {
    const baseProject = transformProject(ProjectSchema.parse(validProject));

    expect(getProjectWorkspacePublishMode(baseProject, false)).toMatchObject({
      kind: "unavailable",
    });
    expect(
      getProjectWorkspacePublishMode(
        {
          ...baseProject,
          githubPrEnabled: false,
          repositoryCapability: {
            kind: "github",
            fetchUrl: null,
            pushUrl: "git@github.com:ralphx/ralphx.git",
          },
        },
        false,
      ),
    ).toMatchObject({ kind: "localCommit" });
    expect(
      getProjectWorkspacePublishMode(
        {
          ...baseProject,
          githubPrEnabled: true,
          repositoryCapability: {
            kind: "github",
            fetchUrl: null,
            pushUrl: "git@github.com:ralphx/ralphx.git",
          },
        },
        false,
      ),
    ).toEqual({ kind: "newPr" });
    expect(getProjectWorkspacePublishMode(baseProject, true)).toEqual({
      kind: "persistedPr",
    });
  });

  it("should parse a project with worktree mode", () => {
    const worktreeProject = {
      ...validProject,
      git_mode: "worktree" as const,
      base_branch: "main",
    };
    expect(() => ProjectSchema.parse(worktreeProject)).not.toThrow();
  });

  it("should reject project with empty id", () => {
    expect(() =>
      ProjectSchema.parse({ ...validProject, id: "" })
    ).toThrow();
  });

  it("should reject project with empty name", () => {
    expect(() =>
      ProjectSchema.parse({ ...validProject, name: "" })
    ).toThrow();
  });

  it("should reject project with empty workingDirectory", () => {
    expect(() =>
      ProjectSchema.parse({ ...validProject, working_directory: "" })
    ).toThrow();
  });

  it("should reject project with invalid gitMode", () => {
    expect(() =>
      ProjectSchema.parse({ ...validProject, git_mode: "invalid" })
    ).toThrow();
  });

  it("should reject project with invalid datetime format", () => {
    expect(() =>
      ProjectSchema.parse({ ...validProject, created_at: "not-a-date" })
    ).toThrow();
  });

  it("should reject project missing required fields", () => {
    expect(() => ProjectSchema.parse({})).toThrow();
    expect(() => ProjectSchema.parse({ id: "test" })).toThrow();
  });
});

describe("CreateProjectSchema", () => {
  it("should parse valid create project data", () => {
    const createData = {
      name: "New Project",
      workingDirectory: "/path/to/project",
      gitMode: "worktree" as const,
    };
    expect(() => CreateProjectSchema.parse(createData)).not.toThrow();
  });

  it("should default gitMode to 'worktree'", () => {
    const createData = {
      name: "New Project",
      workingDirectory: "/path/to/project",
    };
    const result = CreateProjectSchema.parse(createData);
    expect(result.gitMode).toBe("worktree");
  });

  it("should accept worktree configuration", () => {
    const createData = {
      name: "New Project",
      workingDirectory: "/path/to/project",
      gitMode: "worktree" as const,
      baseBranch: "main",
    };
    expect(() => CreateProjectSchema.parse(createData)).not.toThrow();
  });

  it("should reject empty name", () => {
    const createData = {
      name: "",
      workingDirectory: "/path/to/project",
    };
    const result = CreateProjectSchema.safeParse(createData);
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0]?.message).toBe("Project name is required");
    }
  });

  it("should reject empty workingDirectory", () => {
    const createData = {
      name: "Test",
      workingDirectory: "",
    };
    const result = CreateProjectSchema.safeParse(createData);
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0]?.message).toBe("Working directory is required");
    }
  });
});

describe("UpdateProjectSchema", () => {
  it("should allow updating just the name", () => {
    const updateData = { name: "Updated Name" };
    expect(() => UpdateProjectSchema.parse(updateData)).not.toThrow();
  });

  it("should allow updating multiple fields", () => {
    const updateData = {
      name: "Updated Name",
      workingDirectory: "/new/path",
      gitMode: "worktree" as const,
    };
    expect(() => UpdateProjectSchema.parse(updateData)).not.toThrow();
  });

  it("should allow empty object (no updates)", () => {
    expect(() => UpdateProjectSchema.parse({})).not.toThrow();
  });

  it("should allow setting nullable fields to null", () => {
    const updateData = {
      baseBranch: null,
    };
    expect(() => UpdateProjectSchema.parse(updateData)).not.toThrow();
  });

  it("should reject empty string for name", () => {
    const updateData = { name: "" };
    expect(() => UpdateProjectSchema.parse(updateData)).toThrow();
  });
});
