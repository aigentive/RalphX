import { useEffect, useState } from "react";

import type { SettingsSectionId } from "./settings-registry";

export type ScheduledJob = {
  frame: number | null;
  timer: number | null;
};

export const sectionModuleLoaders: Record<SettingsSectionId, () => Promise<unknown>> = {
  execution: () => import("./sections/ExecutionSection"),
  "execution-harnesses": () => import("./IdeationHarnessSection"),
  models: () => import("./AgentModelsSection"),
  "global-execution": () => import("./sections/GlobalExecutionSection"),
  review: () => import("./sections/ReviewPolicySection"),
  repository: () => import("./RepositorySettingsSection"),
  "project-analysis": () => import("./ProjectAnalysisSection"),
  "ideation-workflow": () => import("./IdeationSettingsPanel"),
  "ideation-harnesses": () => import("./IdeationHarnessSection"),
  "api-keys": () => import("./ApiKeysSection"),
  "external-mcp": () => import("./ExternalMcpSettingsPanel"),
  accessibility: () => import("./AccessibilitySection"),
};

export function scheduleAfterPaint(callback: () => void): ScheduledJob {
  const job: ScheduledJob = { frame: null, timer: null };
  const run = () => {
    job.timer = null;
    callback();
  };

  if (typeof window.requestAnimationFrame === "function") {
    job.frame = window.requestAnimationFrame(() => {
      job.frame = null;
      job.timer = window.setTimeout(run, 0);
    });
  } else {
    job.timer = window.setTimeout(run, 0);
  }

  return job;
}

export function cancelScheduledJob(job: ScheduledJob | null): void {
  if (!job) {
    return;
  }
  if (job.frame !== null) {
    window.cancelAnimationFrame(job.frame);
  }
  if (job.timer !== null) {
    window.clearTimeout(job.timer);
  }
}

export function useDeferredDialogFrame(isOpen: boolean): boolean {
  const [renderFrame, setRenderFrame] = useState(isOpen);

  useEffect(() => {
    if (isOpen) {
      setRenderFrame(true);
      return undefined;
    }

    const job = scheduleAfterPaint(() => setRenderFrame(false));
    return () => cancelScheduledJob(job);
  }, [isOpen]);

  return isOpen || renderFrame;
}

export function useDeferredHydratedSection(
  isOpen: boolean,
  activeSection: SettingsSectionId,
): boolean {
  const [hydratedSection, setHydratedSection] =
    useState<SettingsSectionId | null>(null);

  useEffect(() => {
    if (!isOpen) {
      const job = scheduleAfterPaint(() => setHydratedSection(null));
      return () => cancelScheduledJob(job);
    }

    const job = scheduleAfterPaint(() => setHydratedSection(activeSection));
    return () => cancelScheduledJob(job);
  }, [activeSection, isOpen]);

  return hydratedSection === activeSection;
}
