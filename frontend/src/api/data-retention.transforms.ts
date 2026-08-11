import type {
  DataRetentionSettingsResponseRaw,
  RetentionCycleReportRaw,
  SizeBudgetPreviewRaw,
} from "./data-retention.schemas";
import type {
  DataRetentionSettingsResponse,
  RetentionCycleReport,
  SizeBudgetPreview,
} from "./data-retention.types";

export function transformDataRetentionSettingsResponse(
  raw: DataRetentionSettingsResponseRaw,
): DataRetentionSettingsResponse {
  return {
    settings: { ...raw.settings },
    recommendedSizeBudgetBytes: raw.recommendedSizeBudgetBytes,
  };
}

export function transformRetentionCycleReport(
  raw: RetentionCycleReportRaw,
): RetentionCycleReport {
  return { ...raw };
}

export function transformSizeBudgetPreview(raw: SizeBudgetPreviewRaw): SizeBudgetPreview {
  return { ...raw };
}
