import type { DatabaseMaintenanceStatsRaw } from "./database-maintenance.schemas";
import type { DatabaseMaintenanceStats } from "./database-maintenance.types";

export function transformDatabaseMaintenanceStats(
  raw: DatabaseMaintenanceStatsRaw,
): DatabaseMaintenanceStats {
  return {
    databaseBytes: raw.database_bytes,
    reclaimableBytes: raw.reclaimable_bytes,
    headroomOk: raw.headroom_ok,
    pendingCompaction: raw.pending_compaction,
    lastCompaction: raw.last_compaction
      ? {
        outcome: raw.last_compaction.outcome,
        reason: raw.last_compaction.reason,
        reclaimedBytes: raw.last_compaction.reclaimed_bytes,
        databaseBytesBefore: raw.last_compaction.database_bytes_before,
        atRfc3339: raw.last_compaction.at_rfc3339,
      }
      : null,
  };
}
