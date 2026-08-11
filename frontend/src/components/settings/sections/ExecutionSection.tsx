import type { ExecutionSettings } from "@/types/settings";
import {
  NumberSettingRow,
  SettingsSection,
  ToggleSettingRow,
} from "../SettingsView.shared";

interface ExecutionSectionProps {
  settings: ExecutionSettings;
  onChange: (settings: Partial<ExecutionSettings>) => void;
  disabled: boolean;
  embedded?: boolean;
  content?: "all" | "capacity" | "workspace";
}

export default function ExecutionSection({
  settings,
  onChange,
  disabled,
  embedded = false,
  content = "all",
}: ExecutionSectionProps) {
  const rows = (
    <>
      {(content === "all" || content === "capacity") && (
        <>
          <NumberSettingRow
            id="max-concurrent-tasks"
            label="Max Concurrent Tasks"
            description="Maximum number of tasks to run simultaneously (1-10)"
            value={settings.max_concurrent_tasks}
            min={1}
            max={10}
            step={1}
            unit=""
            disabled={disabled}
            onChange={(value) => onChange({ max_concurrent_tasks: value })}
          />
          <NumberSettingRow
            id="project-ideation-max"
            label="Project Ideation Cap"
            description="Maximum concurrent ideation and verification sessions for this project (0-10)"
            value={settings.project_ideation_max}
            min={0}
            max={10}
            step={1}
            unit=""
            disabled={disabled}
            onChange={(value) => onChange({ project_ideation_max: value })}
          />
        </>
      )}
      {(content === "all" || content === "workspace") && (
        <>
          <ToggleSettingRow
            id="agent-workspace-pr-autofix-default"
            label="Default Autofix CI & Reviews"
            description="RalphX monitors this PR for failing checks and review feedback, then publishes follow-up fixes from the workspace automatically."
            checked={settings.agent_workspace_pr_autofix_default}
            disabled={disabled}
            onChange={(checked) =>
              onChange({ agent_workspace_pr_autofix_default: checked })
            }
          />
          <ToggleSettingRow
            id="agent-workspace-pr-auto-merge-default"
            label="Default GitHub auto-merge"
            description="RalphX asks GitHub to merge the PR after required checks and review requirements pass."
            checked={settings.agent_workspace_pr_auto_merge_default}
            disabled={disabled}
            onChange={(checked) =>
              onChange({ agent_workspace_pr_auto_merge_default: checked })
            }
          />
        </>
      )}
    </>
  );
  return embedded ? rows : (
    <SettingsSection>
      {rows}
    </SettingsSection>
  );
}
