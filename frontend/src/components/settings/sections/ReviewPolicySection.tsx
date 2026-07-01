import { ShieldCheck } from "lucide-react";
import { useReviewSettings, useUpdateReviewSettings } from "@/hooks/useReviewSettings";
import {
  NumberSettingRow,
  SectionCard,
  ToggleSettingRow,
} from "../SettingsView.shared";

export default function ReviewPolicySection() {
  const { data: settings, isLoading } = useReviewSettings();
  const { mutate: updateSettings, isPending } = useUpdateReviewSettings();

  const disabled = isLoading || isPending;

  if (isLoading || !settings) {
    return null;
  }

  return (
    <SectionCard
      icon={
        <ShieldCheck className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />
      }
      title="Review Policy"
      description="Configure global review policy for all projects"
    >
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
    </SectionCard>
  );
}
