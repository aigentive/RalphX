import { describe, it, expect } from "vitest";
import {
  ProjectCandidateResponseSchema,
  transformProjectCandidate,
} from "./project-candidate";

describe("ProjectCandidateResponseSchema / transformProjectCandidate", () => {
  it("parses and transforms each simple verdict", () => {
    const cases: Array<[unknown, unknown]> = [
      [{ kind: "not_found" }, { kind: "notFound" }],
      [{ kind: "not_a_directory" }, { kind: "notADirectory" }],
      [{ kind: "empty_directory" }, { kind: "emptyDirectory" }],
      [
        { kind: "non_empty_non_repo", entry_count: 3 },
        { kind: "nonEmptyNonRepo", entryCount: 3 },
      ],
      [
        { kind: "nested_in_repository", repository_root: "/repo" },
        { kind: "nestedInRepository", repositoryRoot: "/repo" },
      ],
      [
        { kind: "detached_head", repository_root: "/repo" },
        { kind: "detachedHead", repositoryRoot: "/repo" },
      ],
      [
        { kind: "inspection_failed", message: "boom" },
        { kind: "inspectionFailed", message: "boom" },
      ],
    ];

    for (const [raw, expected] of cases) {
      const parsed = ProjectCandidateResponseSchema.parse(raw);
      expect(transformProjectCandidate(parsed)).toEqual(expected);
    }
  });

  it("parses and transforms a repository verdict with capability and duplicate registration", () => {
    const raw = {
      kind: "repository",
      repository_root: "/Users/dev/my-app",
      current_branch: "main",
      default_branch: "main",
      branches: ["main", "develop"],
      has_commits: true,
      is_dirty: true,
      capability: {
        kind: "github",
        fetch_url: "https://github.com/o/r.git",
        push_url: "https://github.com/o/r.git",
      },
      already_registered_as: { id: "proj-1", name: "My App" },
    };

    const parsed = ProjectCandidateResponseSchema.parse(raw);
    expect(transformProjectCandidate(parsed)).toEqual({
      kind: "repository",
      repositoryRoot: "/Users/dev/my-app",
      currentBranch: "main",
      defaultBranch: "main",
      branches: ["main", "develop"],
      hasCommits: true,
      isDirty: true,
      capability: {
        kind: "github",
        fetchUrl: "https://github.com/o/r.git",
        pushUrl: "https://github.com/o/r.git",
      },
      alreadyRegisteredAs: { id: "proj-1", name: "My App" },
    });
  });

  it("rejects an unknown kind", () => {
    expect(() => ProjectCandidateResponseSchema.parse({ kind: "bogus" })).toThrow();
  });
});
