export interface CompactionRecord {
  /** "compacted" | "skipped" | "error" */
  outcome: string;
  /** Skip reason or failing phase. */
  reason: string | null;
  reclaimedBytes: number | null;
  databaseBytesBefore: number;
  atRfc3339: string;
}

export interface DatabaseMaintenanceStats {
  databaseBytes: number;
  reclaimableBytes: number;
  headroomOk: boolean;
  pendingCompaction: boolean;
  lastCompaction: CompactionRecord | null;
}
