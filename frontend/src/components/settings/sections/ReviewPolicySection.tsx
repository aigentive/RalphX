import { useReviewSettings, useUpdateReviewSettings } from "@/hooks/useReviewSettings";
import {
  NumberSettingRow,
  SettingsSection,
  ToggleSettingRow,
} from "../SettingsView.shared";

export default function ReviewPolicySection({ embedded = false }: { embedded?: boolean }) {
  const { data: settings, isLoading } = useReviewSettings();
  const { mutate: updateSettings, isPending } = useUpdateReviewSettings();

  const disabled = isLoading || isPending;

  if (isLoading || !settings) {
    return null;
  }

  const rows = (
    <>
      <ToggleSettingRow
        id="require-human-review"
        label="Require Human Review"
        description="Require human review before a task is approved"
        checked={settings.require_human_review}
        disabled={disabled}
        onChange={() =>
          updateSettings({ requireHumanReview: !settings.require_human_review })
        }
      />
      <ToggleSettingRow
        id="run-task-validations"
        label="Run Task Validations"
        description="Allow execution agents to run backend-managed validation commands"
        checked={settings.run_task_validations}
        disabled={disabled}
        onChange={() =>
          updateSettings({ runTaskValidations: !settings.run_task_validations })
        }
      />
      <NumberSettingRow
        id="max-fix-attempts"
        label="Max Fix Attempts"
        description="Maximum times AI can attempt fixes before escalating"
        value={settings.max_fix_attempts}
        min={1}
        max={10}
        step={1}
        unit=""
        disabled={disabled}
        onChange={(value) => updateSettings({ maxFixAttempts: value })}
      />
      <NumberSettingRow
        id="max-revision-cycles"
        label="Max Revision Cycles"
        description="Maximum revision cycles before moving to backlog"
        value={settings.max_revision_cycles}
        min={1}
        max={10}
        step={1}
        unit=""
        disabled={disabled}
        onChange={(value) => updateSettings({ maxRevisionCycles: value })}
      />
    </>
  );
  return embedded ? rows : (
    <SettingsSection>
      {rows}
    </SettingsSection>
  );
}
