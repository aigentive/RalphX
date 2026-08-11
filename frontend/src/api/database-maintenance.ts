import { typedInvokeWithTransform, typedInvoke, TauriVoidSchema } from "@/lib/tauri";
import { DatabaseMaintenanceStatsSchema } from "./database-maintenance.schemas";
import { transformDatabaseMaintenanceStats } from "./database-maintenance.transforms";
import type { DatabaseMaintenanceStats } from "./database-maintenance.types";

export const databaseMaintenanceApi = {
  getStats: (): Promise<DatabaseMaintenanceStats> => typedInvokeWithTransform(
    "get_database_maintenance_stats", {}, DatabaseMaintenanceStatsSchema, transformDatabaseMaintenanceStats,
  ),
  setPending: (pending: boolean): Promise<void> => typedInvoke(
    "set_database_compaction_pending", { input: { pending } }, TauriVoidSchema,
  ).then(() => undefined),
} as const;

export type { CompactionRecord, DatabaseMaintenanceStats } from "./database-maintenance.types";
