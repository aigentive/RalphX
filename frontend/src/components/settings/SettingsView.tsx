/**
 * SettingsView - Slim content dispatcher for project settings sections
 *
 * Renders settings sections without a page shell — intended for use inside
 * SettingsDialog (which provides its own header, left rail, and scroll area).
 */

import { useState, useCallback, useEffect } from "react";
import type {
  ProjectSettings,
  ExecutionSettings,
} from "@/types/settings";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { IdeationSettingsPanel } from "./IdeationSettingsPanel";
import { IdeationEffortSection } from "./IdeationEffortSection";
import { IdeationModelSection } from "./IdeationModelSection";
import { RepositorySettingsSection } from "./RepositorySettingsSection";
import { ProjectAnalysisSection } from "./ProjectAnalysisSection";
import { ApiKeysSection } from "./ApiKeysSection";
import { AppPreferencesSection } from "./AppPreferencesSection";
import {
  SettingsSkeleton,
  ErrorBanner,
} from "./SettingsView.shared";
import ExecutionSection from "./sections/ExecutionSection";
import ReviewPolicySection from "./sections/ReviewPolicySection";
import GlobalExecutionSection from "./sections/GlobalExecutionSection";
import WorkspaceReviewSection from "./sections/WorkspaceReviewSection";

// ============================================================================
// Main Component
// ============================================================================

export interface SettingsViewProps {
  /** Initial settings (if undefined, uses defaults) */
  initialSettings?: ProjectSettings;
  /** Whether to show loading state */
  isLoading?: boolean;
  /** Whether settings are being saved */
  isSaving?: boolean;
  /** Error message to display */
  error?: string | null;
  /** Callback when settings change */
  onSettingsChange?: (settings: ProjectSettings) => void;
}

export function SettingsView({
  initialSettings,
  isLoading = false,
  isSaving = false,
  error = null,
  onSettingsChange,
}: SettingsViewProps) {
  const [settings, setSettings] = useState<ProjectSettings>(
    initialSettings ?? DEFAULT_PROJECT_SETTINGS
  );
  const [dismissedError, setDismissedError] = useState(false);

  // Sync internal state when initialSettings prop changes (e.g., project switch)
  useEffect(() => {
    if (initialSettings) {
      setSettings(initialSettings);
    }
  }, [initialSettings]);

  const handleExecutionChange = useCallback(
    (changes: Partial<ExecutionSettings>) => {
      setSettings((prev) => {
        const updated = {
          ...prev,
          execution: { ...prev.execution, ...changes },
        };
        onSettingsChange?.(updated);
        return updated;
      });
    },
    [onSettingsChange]
  );

  const handleDismissError = useCallback(() => {
    setDismissedError(true);
  }, []);

  // Reset dismissed state when error changes
  const showError = error && !dismissedError;

  if (isLoading) {
    return <SettingsSkeleton />;
  }

  return (
    <div
      data-testid="settings-view"
      className="space-y-6"
    >
      {showError && (
        <ErrorBanner error={error} onDismiss={handleDismissError} />
      )}
      <ExecutionSection
        settings={settings.execution}
        onChange={handleExecutionChange}
        disabled={isSaving}
      />
      <WorkspaceReviewSection />
      <ReviewPolicySection />
      <GlobalExecutionSection />
      <RepositorySettingsSection />
      <ProjectAnalysisSection />
      <IdeationSettingsPanel />
      <IdeationEffortSection />
      <IdeationModelSection />
      <ApiKeysSection />
      <AppPreferencesSection />
    </div>
  );
}
