import { z } from "zod";

// The Rust structs carry `#[serde(rename_all = "camelCase")]`, so the wire shape is
// already camelCase — unlike `database-maintenance`, which serializes snake_case.
export const DataRetentionSettingsSchema = z.object({
  enabled: z.boolean(),
  days: z.number(),
  archivedDays: z.number(),
  batchRows: z.number(),
  sizeBudgetBytes: z.number().nullable(),
  sizeBudgetConfirmedAt: z.string().nullable(),
  seededPristine: z.boolean(),
  sizeBudgetAdvised: z.boolean(),
  lastRunAt: z.string().nullable(),
  lastRunPrunedRows: z.number().nullable(),
  lastRunPayloadBytes: z.number().nullable(),
  lastRunPayloadRows: z.number().nullable(),
  updatedAt: z.string(),
});

export const DataRetentionSettingsResponseSchema = z.object({
  settings: DataRetentionSettingsSchema,
  recommendedSizeBudgetBytes: z.number(),
});

export const RetentionSkipReasonSchema = z.enum([
  "retention_disabled",
  "size_budget_not_configured",
  "size_budget_unconfirmed",
  "already_under_budget",
]);

export const RetentionCycleReportSchema = z.object({
  prunedRows: z.number(),
  payloadBytesAfter: z.number().nullable(),
  payloadRowsAfter: z.number().nullable(),
  databaseBytesAfter: z.number(),
  reclaimableHintBytes: z.number(),
  compactionRecommended: z.boolean(),
  sizeBudgetAdvised: z.boolean(),
  sizeBudgetActive: z.boolean(),
  skippedReason: RetentionSkipReasonSchema.nullable(),
});

export const SizeBudgetPreviewSchema = z.object({
  rows: z.number(),
  bytes: z.number(),
  cutCreatedAt: z.string().nullable(),
});

export type DataRetentionSettingsRaw = z.infer<typeof DataRetentionSettingsSchema>;
export type DataRetentionSettingsResponseRaw = z.infer<
  typeof DataRetentionSettingsResponseSchema
>;
export type RetentionCycleReportRaw = z.infer<typeof RetentionCycleReportSchema>;
export type SizeBudgetPreviewRaw = z.infer<typeof SizeBudgetPreviewSchema>;
