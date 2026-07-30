import { z } from "zod";

export const DatabaseMaintenanceStatsSchema = z.object({
  database_bytes: z.number(),
  reclaimable_bytes: z.number(),
  headroom_ok: z.boolean(),
  pending_compaction: z.boolean(),
});

export type DatabaseMaintenanceStatsRaw = z.infer<typeof DatabaseMaintenanceStatsSchema>;
