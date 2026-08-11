export interface DataRetentionSettings {
  enabled: boolean;
  days: number;
  archivedDays: number;
  batchRows: number;
  /** `null` means size-based pruning is off. This is the shipped state. */
  sizeBudgetBytes: number | null;
  /** Server-recorded consent. Without it a budget is inert. */
  sizeBudgetConfirmedAt: string | null;
  seededPristine: boolean;
  /** Payload data is large enough that a size budget would reclaim meaningful space. */
  sizeBudgetAdvised: boolean;
  lastRunAt: string | null;
  lastRunPrunedRows: number | null;
  lastRunPayloadBytes: number | null;
  lastRunPayloadRows: number | null;
  updatedAt: string;
}

export interface DataRetentionSettingsResponse {
  settings: DataRetentionSettings;
  /** Prefills the size-budget control; never an active cap. */
  recommendedSizeBudgetBytes: number;
}

export interface DataRetentionPolicyInput {
  enabled: boolean;
  days: number;
  archivedDays: number;
  batchRows: number;
  sizeBudgetBytes: number | null;
  /** Explicit user consent. The confirmation timestamp is stamped server-side. */
  sizeBudgetConfirmed: boolean;
}

export type RetentionSkipReason =
  | "retention_disabled"
  | "size_budget_not_configured"
  | "size_budget_unconfirmed"
  | "already_under_budget";

export interface RetentionCycleReport {
  prunedRows: number;
  payloadBytesAfter: number | null;
  payloadRowsAfter: number | null;
  databaseBytesAfter: number;
  reclaimableHintBytes: number;
  compactionRecommended: boolean;
  sizeBudgetAdvised: boolean;
  sizeBudgetActive: boolean;
  skippedReason: RetentionSkipReason | null;
}

export interface SizeBudgetPreview {
  rows: number;
  bytes: number;
  /** Everything created before this timestamp would be deleted. */
  cutCreatedAt: string | null;
}
