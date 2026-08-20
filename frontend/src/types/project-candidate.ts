// Project candidate types and Zod schema
// Mirrors src-tauri/src/commands/project_probe_commands.rs::ProjectCandidate (snake_case, tag="kind")

import { z } from "zod";
import {
  RepositoryCapabilityResponseSchema,
  transformRepositoryCapability,
  type RepositoryCapability,
} from "@/types/project";

const RegisteredProjectRefResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
});

export interface RegisteredProjectRef {
  id: string;
  name: string;
}

/**
 * Backend response schema - expects snake_case, externally tagged on `kind`
 */
export const ProjectCandidateResponseSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("not_found") }),
  z.object({ kind: z.literal("not_a_directory") }),
  z.object({ kind: z.literal("empty_directory") }),
  z.object({ kind: z.literal("non_empty_non_repo"), entry_count: z.number() }),
  z.object({ kind: z.literal("nested_in_repository"), repository_root: z.string() }),
  z.object({ kind: z.literal("detached_head"), repository_root: z.string() }),
  z.object({
    kind: z.literal("repository"),
    repository_root: z.string(),
    current_branch: z.string(),
    default_branch: z.string().nullable(),
    branches: z.array(z.string()),
    has_commits: z.boolean(),
    is_dirty: z.boolean(),
    capability: RepositoryCapabilityResponseSchema,
    already_registered_as: RegisteredProjectRefResponseSchema.nullable(),
  }),
  z.object({ kind: z.literal("inspection_failed"), message: z.string() }),
]);

/**
 * Frontend ProjectCandidate union - uses camelCase
 */
export type ProjectCandidate =
  | { kind: "notFound" }
  | { kind: "notADirectory" }
  | { kind: "emptyDirectory" }
  | { kind: "nonEmptyNonRepo"; entryCount: number }
  | { kind: "nestedInRepository"; repositoryRoot: string }
  | { kind: "detachedHead"; repositoryRoot: string }
  | {
      kind: "repository";
      repositoryRoot: string;
      currentBranch: string;
      defaultBranch: string | null;
      branches: string[];
      hasCommits: boolean;
      isDirty: boolean;
      capability: RepositoryCapability;
      alreadyRegisteredAs: RegisteredProjectRef | null;
    }
  | { kind: "inspectionFailed"; message: string };

/**
 * Transform snake_case backend response to camelCase frontend type
 */
export function transformProjectCandidate(
  response: z.infer<typeof ProjectCandidateResponseSchema>
): ProjectCandidate {
  switch (response.kind) {
    case "not_found":
      return { kind: "notFound" };
    case "not_a_directory":
      return { kind: "notADirectory" };
    case "empty_directory":
      return { kind: "emptyDirectory" };
    case "non_empty_non_repo":
      return { kind: "nonEmptyNonRepo", entryCount: response.entry_count };
    case "nested_in_repository":
      return { kind: "nestedInRepository", repositoryRoot: response.repository_root };
    case "detached_head":
      return { kind: "detachedHead", repositoryRoot: response.repository_root };
    case "repository":
      return {
        kind: "repository",
        repositoryRoot: response.repository_root,
        currentBranch: response.current_branch,
        defaultBranch: response.default_branch,
        branches: response.branches,
        hasCommits: response.has_commits,
        isDirty: response.is_dirty,
        capability: transformRepositoryCapability(response.capability),
        alreadyRegisteredAs: response.already_registered_as,
      };
    case "inspection_failed":
      return { kind: "inspectionFailed", message: response.message };
  }
}
