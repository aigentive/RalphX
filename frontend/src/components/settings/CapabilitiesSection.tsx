import { toast } from "sonner";

import {
  useFeatureFlags,
  useUpdateFeatureFlags,
} from "@/hooks/useFeatureFlags";

import { ToggleSettingRow } from "./SettingsView.shared";

export function CapabilitiesSection() {
  const { data: featureFlags, isPlaceholderData } = useFeatureFlags();
  const updateFeatureFlags = useUpdateFeatureFlags();
  const disabled = isPlaceholderData || updateFeatureFlags.isPending;

  const update = (
    input:
      | { agentConversationTeam: boolean }
      | { agentConversationWorkflows: boolean },
  ) => {
    updateFeatureFlags.mutate(input, {
      onError: (error) => {
        toast.error("Could not update Agent capabilities", {
          description: error instanceof Error ? error.message : String(error),
        });
      },
    });
  };

  return (
    <section aria-label="Capabilities" className="space-y-4">
      <div className="space-y-1">
        <h2 className="text-sm font-semibold text-[var(--text-primary)]">
          Agent conversation capabilities
        </h2>
        <p className="text-xs leading-relaxed text-[var(--text-secondary)]">
          These experimental orchestration modes are opt-in and apply across the
          app.
        </p>
      </div>

      <ToggleSettingRow
        id="agent-conversation-team"
        label="Team"
        description="Enable RalphX-native multi-agent Team mode in Agent conversations. Experimental."
        checked={
          !isPlaceholderData && (featureFlags.agentConversationTeam ?? false)
        }
        disabled={disabled}
        onChange={(agentConversationTeam) => update({ agentConversationTeam })}
      />
      <ToggleSettingRow
        id="agent-conversation-workflows"
        label="Workflows"
        description="Enable agent-generated scripted workflows in Agent conversations. Experimental."
        checked={
          !isPlaceholderData &&
          (featureFlags.agentConversationWorkflows ?? false)
        }
        disabled={disabled}
        onChange={(agentConversationWorkflows) =>
          update({ agentConversationWorkflows })
        }
      />

      <p className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-xs leading-relaxed text-[var(--text-secondary)]">
        Codex Ultra is availability-driven and selected per conversation. It
        uses provider-native subagents and can dramatically increase usage.
      </p>
    </section>
  );
}
