import { z } from "zod";

import { STARTUP_STAGES } from "./startup.types";

export const StartupProgressSchema = z.object({
  completed_units: z.number().int().nonnegative(),
  total_units: z.number().int().nonnegative(),
});

export const StartupStatusSchema = z.object({
  boot_id: z.string().min(1),
  attempt_id: z.number().int().positive(),
  stage: z.enum(STARTUP_STAGES),
  started_at: z.string().min(1),
  stage_started_at: z.string().min(1),
  completed_at: z.string().min(1).nullable().optional(),
  app_state_ready: z.boolean(),
  runtime_ready: z.boolean(),
  background_complete: z.boolean(),
  retry_allowed: z.boolean(),
  progress: StartupProgressSchema.nullable().optional(),
  message_code: z.string().min(1),
  failure_code: z.string().min(1).nullable().optional(),
  diagnostic_summary: z.string().min(1).nullable().optional(),
});

export const StartupDiagnosticsSchema = z.object({
  attempt_id: z.number().int().positive(),
  stage: z.enum(STARTUP_STAGES),
  message_code: z.string().min(1),
  failure_code: z.string().min(1).nullable().optional(),
  can_retry: z.boolean(),
}).strict();
