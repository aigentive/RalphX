
import { useReviewSettings, useUpdateReviewSettings } from "@/hooks/useReviewSettings";

import {
  SettingsSection,
  ToggleSettingRow,
} from "../SettingsView.shared";

const ISSUE_RULES = [
  "Agents register drift, blockers, and decision points as Issues on the origin Agent conversation.",
  "Blocking work still uses the task execution state machine: Blocked, Escalated, review, and merge attention states remain authoritative.",
  "When auto-create is enabled, eligible Issues create or reuse a visible follow-up Agent conversation.",
  "When auto-create is disabled, Issues stay open until the user creates a follow-up, resolves, or dismisses them.",
];

export default function AutonomyPolicySection({ embedded = false }: { embedded?: boolean }) {
  const { data: settings, isLoading } = useReviewSettings();
  const { mutate: updateSettings, isPending } = useUpdateReviewSettings();

  const disabled = isLoading || isPending;

  if (isLoading || !settings) {
    return null;
  }

  const rows = (
    <>
      <ToggleSettingRow
        id="auto-create-followup-agent-conversation"
        label="Auto-create follow-up Agent conversations"
        description="Let eligible drift and decision Issues create follow-up Agent conversations automatically"
        checked={settings.auto_create_followup_agent_conversation}
        disabled={disabled}
        onChange={(checked) =>
          updateSettings({ autoCreateFollowupAgentConversation: checked })
        }
      />
      <div
        className="rounded-md border p-3"
        style={{
          backgroundColor: "var(--bg-base)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: 1,
        }}
      >
        <h3 className="text-sm font-semibold">Agent issue rules</h3>
        <ul className="mt-2 space-y-1.5 text-xs leading-relaxed">
          {ISSUE_RULES.map((rule) => (
            <li key={rule} className="flex gap-2">
              <span aria-hidden="true" style={{ color: "var(--accent-primary)" }}>-</span>
              <span style={{ color: "var(--text-secondary)" }}>{rule}</span>
            </li>
          ))}
        </ul>
      </div>
    </>
  );
  return embedded ? rows : (
    <SettingsSection>
      {rows}
    </SettingsSection>
  );
}
