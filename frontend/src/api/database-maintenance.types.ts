export interface DatabaseMaintenanceStats {
  databaseBytes: number;
  reclaimableBytes: number;
  headroomOk: boolean;
  pendingCompaction: boolean;
}
