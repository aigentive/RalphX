// Clone job types and Zod schemas
// Mirrors src-tauri/src/application/clone_job_registry.rs::CloneJobStatus (tag="state", camelCase)
// and src-tauri/src/application/clone_job_runner.rs event payloads (project:clone_*)

import { z } from "zod";
import {
  RepositoryCapabilityResponseSchema,
  transformRepositoryCapability,
  type RepositoryCapability,
} from "@/types/project";

/**
 * Phases reported by `git clone --progress`, in the order Git emits them.
 * Mirrors src-tauri/src/application/git_service/clone.rs::ClonePhase (snake_case values).
 */
export const ClonePhaseSchema = z.enum([
  "connecting",
  "counting",
  "compressing",
  "receiving",
  "resolving",
  "checking_out",
]);
export type ClonePhase = z.infer<typeof ClonePhaseSchema>;

const RunningStatusResponseSchema = z.object({
  state: z.literal("running"),
  phase: ClonePhaseSchema.nullable(),
  percent: z.number().nullable(),
});

const CompletedStatusResponseSchema = z.object({
  state: z.literal("completed"),
  destination: z.string(),
  defaultBranch: z.string().nullable(),
  capability: RepositoryCapabilityResponseSchema,
});

const FailedStatusResponseSchema = z.object({
  state: z.literal("failed"),
  code: z.string(),
  message: z.string(),
  cleanedUp: z.boolean(),
});

const CancelledStatusResponseSchema = z.object({
  state: z.literal("cancelled"),
  cleanedUp: z.boolean(),
});

const UnknownStatusResponseSchema = z.object({ state: z.literal("unknown") });

/**
 * Backend response schema for `get_clone_job_status`.
 * `capability` is snake_case-tagged internally; everything else is already camelCase.
 */
export const CloneJobStatusResponseSchema = z.discriminatedUnion("state", [
  RunningStatusResponseSchema,
  CompletedStatusResponseSchema,
  FailedStatusResponseSchema,
  CancelledStatusResponseSchema,
  UnknownStatusResponseSchema,
]);
export type CloneJobStatusResponse = z.infer<typeof CloneJobStatusResponseSchema>;

/**
 * Frontend CloneJobStatus union — fully camelCase, including `capability`.
 */
export type CloneJobStatus =
  | { state: "running"; phase: ClonePhase | null; percent: number | null }
  | {
      state: "completed";
      destination: string;
      defaultBranch: string | null;
      capability: RepositoryCapability;
    }
  | { state: "failed"; code: string; message: string; cleanedUp: boolean }
  | { state: "cancelled"; cleanedUp: boolean }
  | { state: "unknown" };

/**
 * Transform the raw backend status into the camelCase frontend shape.
 * Also accepts the `project:clone_*` terminal event payloads, which carry the
 * same fields plus a `jobId` this function ignores.
 */
export function transformCloneJobStatus(
  response: CloneJobStatusResponse | (CloneJobStatusResponse & { jobId: string })
): CloneJobStatus {
  switch (response.state) {
    case "running":
      return { state: "running", phase: response.phase, percent: response.percent };
    case "completed":
      return {
        state: "completed",
        destination: response.destination,
        defaultBranch: response.defaultBranch,
        capability: transformRepositoryCapability(response.capability),
      };
    case "failed":
      return {
        state: "failed",
        code: response.code,
        message: response.message,
        cleanedUp: response.cleanedUp,
      };
    case "cancelled":
      return { state: "cancelled", cleanedUp: response.cleanedUp };
    case "unknown":
      return { state: "unknown" };
  }
}

/**
 * `project:clone_progress` event payload — already fully camelCase.
 */
export const CloneProgressEventSchema = z.object({
  jobId: z.string(),
  phase: ClonePhaseSchema,
  percent: z.number().nullable(),
  received: z.number().nullable(),
  total: z.number().nullable(),
  line: z.string(),
});
export type CloneProgressEvent = z.infer<typeof CloneProgressEventSchema>;

/**
 * Terminal event payload schemas — each is the matching status variant plus `jobId`.
 */
export const CloneCompletedEventSchema = CompletedStatusResponseSchema.extend({
  jobId: z.string(),
});
export const CloneFailedEventSchema = FailedStatusResponseSchema.extend({
  jobId: z.string(),
});
export const CloneCancelledEventSchema = CancelledStatusResponseSchema.extend({
  jobId: z.string(),
});

export type CloneCompletedEvent = z.infer<typeof CloneCompletedEventSchema>;
export type CloneFailedEvent = z.infer<typeof CloneFailedEventSchema>;
export type CloneCancelledEvent = z.infer<typeof CloneCancelledEventSchema>;
