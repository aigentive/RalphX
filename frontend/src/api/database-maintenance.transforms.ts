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
  };
}
