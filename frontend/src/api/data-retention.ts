import { typedInvokeWithTransform } from "@/lib/tauri";
import {
  DataRetentionSettingsResponseSchema,
  RetentionCycleReportSchema,
  SizeBudgetPreviewSchema,
} from "./data-retention.schemas";
import {
  transformDataRetentionSettingsResponse,
  transformRetentionCycleReport,
  transformSizeBudgetPreview,
} from "./data-retention.transforms";
import type {
  DataRetentionPolicyInput,
  DataRetentionSettingsResponse,
  RetentionCycleReport,
  SizeBudgetPreview,
} from "./data-retention.types";

export const dataRetentionApi = {
  getSettings: (): Promise<DataRetentionSettingsResponse> => typedInvokeWithTransform(
    "get_data_retention_settings", {}, DataRetentionSettingsResponseSchema,
    transformDataRetentionSettingsResponse,
  ),
  updateSettings: (input: DataRetentionPolicyInput): Promise<DataRetentionSettingsResponse> =>
    typedInvokeWithTransform(
      "update_data_retention_settings", { input }, DataRetentionSettingsResponseSchema,
      transformDataRetentionSettingsResponse,
    ),
  runNow: (): Promise<RetentionCycleReport> => typedInvokeWithTransform(
    "run_data_retention_now", {}, RetentionCycleReportSchema, transformRetentionCycleReport,
  ),
  /** Read-only. Shows exactly what enabling or lowering a size budget would delete. */
  previewSizeBudget: (budgetBytes: number): Promise<SizeBudgetPreview> =>
    typedInvokeWithTransform(
      "preview_data_retention_size_budget", { input: { budgetBytes } }, SizeBudgetPreviewSchema,
      transformSizeBudgetPreview,
    ),
} as const;

export type {
  DataRetentionPolicyInput,
  DataRetentionSettings,
  DataRetentionSettingsResponse,
  RetentionCycleReport,
  RetentionSkipReason,
  SizeBudgetPreview,
} from "./data-retention.types";
