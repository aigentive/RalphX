
import type { ProjectSettings } from "@/types/settings";

import { SettingsSection } from "../SettingsView.shared";
import ExecutionSection from "./ExecutionSection";
import GlobalExecutionSection from "./GlobalExecutionSection";

export default function CapacitySettingsSection({
  settings,
  disabled,
  onSettingsChange,
}: {
  settings: ProjectSettings;
  disabled: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
}) {
  return (
    <SettingsSection>
      <ExecutionSection
        settings={settings.execution}
        onChange={(changes) =>
          onSettingsChange({
            ...settings,
            execution: { ...settings.execution, ...changes },
          })
        }
        disabled={disabled}
        content="capacity"
        embedded
      />
      <div className="mt-5 border-t border-[var(--border-subtle)] pt-3">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)]">
          Global capacity
        </h3>
        <GlobalExecutionSection embedded />
      </div>
    </SettingsSection>
  );
}
