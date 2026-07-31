/**
 * SettingsView - Slim content dispatcher for project settings sections
 *
 * Renders settings sections without a page shell — intended for use inside
 * SettingsDialog (which provides its own header, left rail, and scroll area).
 */

import { useState, useCallback, useEffect } from "react";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import type { ProjectSettings } from "@/types/settings";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { IdeationEffortSection } from "./IdeationEffortSection";
import { IdeationModelSection } from "./IdeationModelSection";
import { RepositorySettingsSection } from "./RepositorySettingsSection";
import { ProjectAnalysisSection } from "./ProjectAnalysisSection";
import { ApiKeysSection } from "./ApiKeysSection";
import {
  SettingsSkeleton,
  ErrorBanner,
} from "./SettingsView.shared";
import CapacitySettingsSection from "./sections/CapacitySettingsSection";
import PlanningSettingsSection from "./sections/PlanningSettingsSection";
import TasksSettingsSection from "./sections/TasksSettingsSection";
import WorkspaceSettingsSection from "./sections/WorkspaceSettingsSection";

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
  const ideationController = useIdeationSettings(!isLoading);

  // Sync internal state when initialSettings prop changes (e.g., project switch)
  useEffect(() => {
    if (initialSettings) {
      setSettings(initialSettings);
    }
  }, [initialSettings]);

  const handleSettingsChange = useCallback(
    (updated: ProjectSettings) => {
      setSettings(updated);
      onSettingsChange?.(updated);
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
      <TasksSettingsSection controller={ideationController} />
      <PlanningSettingsSection controller={ideationController} />
      <WorkspaceSettingsSection settings={settings} disabled={isSaving} onSettingsChange={handleSettingsChange} />
      <CapacitySettingsSection settings={settings} disabled={isSaving} onSettingsChange={handleSettingsChange} />
      <RepositorySettingsSection />
      <ProjectAnalysisSection />
      <IdeationEffortSection />
      <IdeationModelSection />
      <ApiKeysSection />
    </div>
  );
}
