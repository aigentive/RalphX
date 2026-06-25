/**
 * GlobalExecutionSection - Manage global concurrency cap across all projects
 * Phase 82: Separate section with its own loading/saving state
 */

import { useState, useCallback, useEffect, useRef } from "react";
import { Globe } from "lucide-react";
import { executionApi } from "@/api/execution";
import type { GlobalExecutionSettingsResponse } from "@/api/execution";
import {
  NumberSettingRow,
  SectionCard,
  ToggleSettingRow,
} from "../SettingsView.shared";

export default function GlobalExecutionSection() {
  const [globalSettings, setGlobalSettings] = useState<GlobalExecutionSettingsResponse | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Load global settings on mount
  useEffect(() => {
    async function loadGlobalSettings() {
      try {
        setIsLoading(true);
        setError(null);
        const settings = await executionApi.getGlobalSettings();
        setGlobalSettings(settings);
      } catch (err) {
        console.error("Failed to load global execution settings:", err);
        setError(err instanceof Error ? err.message : "Failed to load global settings");
        setGlobalSettings({
          globalMaxConcurrent: 20,
          workspaceMaxConcurrent: 10,
          globalIdeationMax: 10,
          allowIdeationBorrowIdleExecution: false,
        });
      } finally {
        setIsLoading(false);
      }
    }
    loadGlobalSettings();
  }, []);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
      }
    };
  }, []);

  const scheduleSave = useCallback((nextSettings: GlobalExecutionSettingsResponse) => {
    setError(null);

    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
    }

    saveTimeoutRef.current = setTimeout(async () => {
      try {
        setIsSaving(true);
        await executionApi.updateGlobalSettings(nextSettings);
      } catch (err) {
        console.error("Failed to save global execution settings:", err);
        setError(err instanceof Error ? err.message : "Failed to save global settings");
      } finally {
        setIsSaving(false);
      }
    }, 300);
  }, []);

  const handleGlobalMaxChange = useCallback((value: number) => {
    setGlobalSettings((prev) => {
      const nextSettings = {
        globalMaxConcurrent: value,
        workspaceMaxConcurrent: prev?.workspaceMaxConcurrent ?? 10,
        globalIdeationMax: prev?.globalIdeationMax ?? 10,
        allowIdeationBorrowIdleExecution:
          prev?.allowIdeationBorrowIdleExecution ?? false,
      };
      scheduleSave(nextSettings);
      return nextSettings;
    });
  }, [scheduleSave]);

  const handleWorkspaceMaxChange = useCallback((value: number) => {
    setGlobalSettings((prev) => {
      const nextSettings = {
        globalMaxConcurrent: prev?.globalMaxConcurrent ?? 20,
        workspaceMaxConcurrent: value,
        globalIdeationMax: prev?.globalIdeationMax ?? 10,
        allowIdeationBorrowIdleExecution:
          prev?.allowIdeationBorrowIdleExecution ?? false,
      };
      scheduleSave(nextSettings);
      return nextSettings;
    });
  }, [scheduleSave]);

  const handleGlobalIdeationMaxChange = useCallback((value: number) => {
    setGlobalSettings((prev) => {
      const nextSettings = {
        globalMaxConcurrent: prev?.globalMaxConcurrent ?? 20,
        workspaceMaxConcurrent: prev?.workspaceMaxConcurrent ?? 10,
        globalIdeationMax: value,
        allowIdeationBorrowIdleExecution:
          prev?.allowIdeationBorrowIdleExecution ?? false,
      };
      scheduleSave(nextSettings);
      return nextSettings;
    });
  }, [scheduleSave]);

  const handleBorrowToggle = useCallback(() => {
    setGlobalSettings((prev) => {
      const nextSettings = {
        globalMaxConcurrent: prev?.globalMaxConcurrent ?? 20,
        workspaceMaxConcurrent: prev?.workspaceMaxConcurrent ?? 10,
        globalIdeationMax: prev?.globalIdeationMax ?? 10,
        allowIdeationBorrowIdleExecution:
          !(prev?.allowIdeationBorrowIdleExecution ?? false),
      };
      scheduleSave(nextSettings);
      return nextSettings;
    });
  }, [scheduleSave]);

  if (isLoading) {
    return (
      <SectionCard
        icon={<Globe className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />}
        title="Global Capacity"
        description="Cross-project concurrency limits"
      >
        <div className="py-4 flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
        </div>
      </SectionCard>
    );
  }

  return (
    <SectionCard
      icon={<Globe className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />}
      title="Global Capacity"
      description="Cross-project concurrency limits"
    >
      {error && (
        <div className="mb-3 px-3 py-2 rounded-md bg-status-error/10 border border-status-error/20 text-status-error text-sm">
          {error}
        </div>
      )}
      <NumberSettingRow
        id="global-max-concurrent"
        label={isSaving ? "Global Max Concurrent (Saving...)" : "Global Max Concurrent"}
        description="Maximum total tasks running across ALL projects (1-50). This cap applies system-wide regardless of per-project settings."
        value={globalSettings?.globalMaxConcurrent ?? 20}
        min={1}
        max={50}
        step={1}
        unit=""
        disabled={isSaving}
        onChange={handleGlobalMaxChange}
      />
      <NumberSettingRow
        id="workspace-max-concurrent"
        label="Workspace Main Agent Cap"
        description="Maximum concurrent workspace conversations across all projects (1-50)"
        value={globalSettings?.workspaceMaxConcurrent ?? 10}
        min={1}
        max={50}
        step={1}
        unit=""
        disabled={isSaving}
        onChange={handleWorkspaceMaxChange}
      />
      <NumberSettingRow
        id="global-ideation-max"
        label="Global Ideation Cap"
        description="Maximum concurrent ideation and verification sessions across all projects (1-50)"
        value={globalSettings?.globalIdeationMax ?? 10}
        min={1}
        max={50}
        step={1}
        unit=""
        disabled={isSaving}
        onChange={handleGlobalIdeationMaxChange}
      />
      <ToggleSettingRow
        id="allow-ideation-borrow-idle-execution"
        label="Allow Ideation Borrowing"
        description="Let lower-priority ideation use idle capacity when no Workspaces or Tasks are waiting"
        checked={globalSettings?.allowIdeationBorrowIdleExecution ?? false}
        disabled={isSaving}
        onChange={handleBorrowToggle}
      />
    </SectionCard>
  );
}
