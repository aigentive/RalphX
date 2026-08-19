// Worktree-parent preflight verdict types and Zod schema
// Mirrors src-tauri/src/commands/project_probe_commands.rs::WorktreeParentVerdict (snake_case, tag="verdict")

import { z } from "zod";

/**
 * Backend response schema - expects snake_case, externally tagged on `verdict`.
 */
export const WorktreeParentVerdictResponseSchema = z.discriminatedUnion("verdict", [
  z.object({ verdict: z.literal("ok"), path: z.string() }),
  z.object({ verdict: z.literal("not_found"), path: z.string() }),
  z.object({ verdict: z.literal("not_a_directory"), path: z.string() }),
  z.object({ verdict: z.literal("inside_repository"), path: z.string() }),
  z.object({ verdict: z.literal("not_writable"), path: z.string() }),
  z.object({ verdict: z.literal("invalid"), message: z.string() }),
]);

export type WorktreeParentVerdictResponse = z.infer<typeof WorktreeParentVerdictResponseSchema>;

/**
 * Frontend WorktreeParentVerdict union - uses camelCase.
 */
export type WorktreeParentVerdict =
  | { verdict: "ok"; path: string }
  | { verdict: "notFound"; path: string }
  | { verdict: "notADirectory"; path: string }
  | { verdict: "insideRepository"; path: string }
  | { verdict: "notWritable"; path: string }
  | { verdict: "invalid"; message: string };

/**
 * Transform snake_case backend response to camelCase frontend type.
 */
export function transformWorktreeParentVerdict(
  response: WorktreeParentVerdictResponse
): WorktreeParentVerdict {
  switch (response.verdict) {
    case "ok":
      return { verdict: "ok", path: response.path };
    case "not_found":
      return { verdict: "notFound", path: response.path };
    case "not_a_directory":
      return { verdict: "notADirectory", path: response.path };
    case "inside_repository":
      return { verdict: "insideRepository", path: response.path };
    case "not_writable":
      return { verdict: "notWritable", path: response.path };
    case "invalid":
      return { verdict: "invalid", message: response.message };
  }
}

/**
 * `not_writable` is advisory only. Every other non-`ok` verdict blocks Create.
 */
export function isWorktreeParentVerdictBlocking(verdict: WorktreeParentVerdict | null): boolean {
  if (!verdict) return false;
  return verdict.verdict !== "ok" && verdict.verdict !== "notWritable";
}

const WORKTREE_PARENT_VERDICT_COPY: Record<
  WorktreeParentVerdict["verdict"],
  { tone: "success" | "warning" | "error"; message: string }
> = {
  ok: { tone: "success", message: "This folder is ready to use for task worktrees." },
  notFound: { tone: "error", message: "This folder doesn't exist yet. Choose a different folder." },
  notADirectory: { tone: "error", message: "That path isn't a folder. Choose a folder instead." },
  insideRepository: {
    tone: "error",
    message:
      "This folder is inside the repository you're adding. Task worktrees need a location outside the repository.",
  },
  notWritable: {
    tone: "warning",
    message: "RalphX may not be able to write here. Task worktrees might fail to create.",
  },
  invalid: { tone: "error", message: "" },
};

/**
 * Plain-language copy for a verdict, keyed by tone for styling.
 */
export function describeWorktreeParentVerdict(
  verdict: WorktreeParentVerdict
): { tone: "success" | "warning" | "error"; message: string } {
  if (verdict.verdict === "invalid") {
    return { tone: "error", message: verdict.message };
  }
  return WORKTREE_PARENT_VERDICT_COPY[verdict.verdict];
}
