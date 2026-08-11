import { z } from "zod";

export const CompactionRecordSchema = z.object({
  outcome: z.string(),
  reason: z.string().nullable(),
  reclaimed_bytes: z.number().nullable(),
  database_bytes_before: z.number(),
  at_rfc3339: z.string(),
});

export const DatabaseMaintenanceStatsSchema = z.object({
  database_bytes: z.number(),
  reclaimable_bytes: z.number(),
  headroom_ok: z.boolean(),
  pending_compaction: z.boolean(),
  last_compaction: CompactionRecordSchema.nullable(),
});

export type CompactionRecordRaw = z.infer<typeof CompactionRecordSchema>;
export type DatabaseMaintenanceStatsRaw = z.infer<typeof DatabaseMaintenanceStatsSchema>;
