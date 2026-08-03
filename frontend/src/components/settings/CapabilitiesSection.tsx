import { toast } from "sonner";

import { Switch } from "@/components/ui/switch";
import {
  useFeatureFlags,
  useUpdateFeatureFlags,
} from "@/hooks/useFeatureFlags";

interface CapabilityCardProps {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}

function CapabilityCard({
  id,
  label,
  description,
  checked,
  disabled,
  onChange,
}: CapabilityCardProps) {
  return (
    <div className="settings-card settings-card--row">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span
            id={`${id}-label`}
            className="text-sm font-semibold text-[var(--text-primary)]"
          >
            {label}
          </span>
          <span className="rounded-[5px] border border-[var(--border-default)] px-1.5 py-px text-[11px] text-[var(--text-muted)]">
            Experimental
          </span>
        </div>
        <p id={`${id}-desc`} className="settings-row__help">
          {description}
        </p>
      </div>
      <Switch
        id={id}
        data-testid={id}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        aria-labelledby={`${id}-label`}
        aria-describedby={`${id}-desc`}
        className="settings-toggle data-[state=checked]:bg-[var(--accent-primary)]"
      />
    </div>
  );
}

export function CapabilitiesSection() {
  const { data: featureFlags, isPlaceholderData } = useFeatureFlags();
  const updateFeatureFlags = useUpdateFeatureFlags();
  const disabled = isPlaceholderData || updateFeatureFlags.isPending;

  const update = (
    input:
      | { agentConversationTeam: boolean }
      | { agentConversationWorkflows: boolean }
      | { agentConversationAutopilot: boolean },
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
    <section
      aria-label="Capabilities"
      className="flex max-w-[820px] flex-col gap-4"
    >
      <CapabilityCard
        id="agent-conversation-autopilot"
        label="Autopilot"
        description="Allow native Agent conversations to plan, create tasks, and start execution with minimal supervision."
        checked={
          !isPlaceholderData &&
          (featureFlags.agentConversationAutopilot ?? false)
        }
        disabled={disabled}
        onChange={(agentConversationAutopilot) =>
          update({ agentConversationAutopilot })
        }
      />
      <CapabilityCard
        id="agent-conversation-team"
        label="Team"
        description="Enable RalphX-native multi-agent Team mode in Agent conversations."
        checked={
          !isPlaceholderData && (featureFlags.agentConversationTeam ?? false)
        }
        disabled={disabled}
        onChange={(agentConversationTeam) => update({ agentConversationTeam })}
      />
      <CapabilityCard
        id="agent-conversation-workflows"
        label="Workflows"
        description="Enable agent-generated scripted workflows in Agent conversations."
        checked={
          !isPlaceholderData &&
          (featureFlags.agentConversationWorkflows ?? false)
        }
        disabled={disabled}
        onChange={(agentConversationWorkflows) =>
          update({ agentConversationWorkflows })
        }
      />
      <div className="settings-card text-xs leading-relaxed text-[var(--text-secondary)]">
        Codex Ultra is availability-driven and selected per conversation. It
        uses provider-native subagents and can dramatically increase usage.
      </div>
    </section>
  );
}
