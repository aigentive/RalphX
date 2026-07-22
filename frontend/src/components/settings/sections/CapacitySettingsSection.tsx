import { Gauge } from "lucide-react";

import type { ProjectSettings } from "@/types/settings";

import { SectionCard } from "../SettingsView.shared";
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
    <SectionCard
      icon={<Gauge className="h-5 w-5" />}
      title="Capacity"
      description="Configure project and global execution and ideation concurrency."
    >
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
    </SectionCard>
  );
}
