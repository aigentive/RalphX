import {
  TauriVoidSchema,
  typedInvoke,
  typedInvokeWithTransform,
} from "@/lib/tauri";

import {
  StartupDiagnosticsSchema,
  StartupStatusSchema,
} from "./startup.schemas";
import {
  transformStartupDiagnostics,
  transformStartupStatus,
} from "./startup.transforms";
import type {
  ReportStartupFrontendMilestoneInput,
  StartupDiagnostics,
  StartupStatus,
} from "./startup.types";

export type {
  ReportStartupFrontendMilestoneInput,
  StartupFrontendMilestone,
  StartupDiagnostics,
  StartupProgress,
  StartupStage,
  StartupStatus,
} from "./startup.types";

export {
  StartupDiagnosticsSchema,
  StartupStatusSchema,
} from "./startup.schemas";
export {
  transformStartupDiagnostics,
  transformStartupStatus,
} from "./startup.transforms";

export const startupApi = {
  getStatus: (): Promise<StartupStatus> =>
    typedInvokeWithTransform(
      "get_startup_status",
      {},
      StartupStatusSchema,
      transformStartupStatus,
    ),

  retry: (): Promise<StartupStatus> =>
    typedInvokeWithTransform(
      "retry_startup",
      {},
      StartupStatusSchema,
      transformStartupStatus,
    ),

  openLogs: async (): Promise<void> => {
    await typedInvoke("open_startup_logs", {}, TauriVoidSchema);
  },

  getDiagnostics: (): Promise<StartupDiagnostics> =>
    typedInvokeWithTransform(
      "get_startup_diagnostics",
      {},
      StartupDiagnosticsSchema,
      transformStartupDiagnostics,
    ),

  reportFrontendMilestone: async (
    input: ReportStartupFrontendMilestoneInput,
  ): Promise<void> => {
    await typedInvoke(
      "report_startup_frontend_milestone",
      { input },
      TauriVoidSchema,
    );
  },
} as const;
