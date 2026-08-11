import { Switch } from "@/components/ui/switch";

interface PersonasEnableToggleProps {
  enabled: boolean;
  pending: boolean;
  onEnabledChange: (enabled: boolean) => void;
}

export function PersonasEnableToggle({
  enabled,
  pending,
  onEnabledChange,
}: PersonasEnableToggleProps) {
  return (
    <div className="settings-card settings-card--row">
      <div className="min-w-0">
        <span
          id="agent-personas-enabled-label"
          className="text-sm font-semibold text-[var(--text-primary)]"
        >
          Enable agent personas
        </span>
        <p id="agent-personas-enabled-desc" className="settings-row__help">
          Conversation-bound behavior profiles. Experimental.
        </p>
      </div>
      <Switch
        id="agent-personas-enabled"
        data-testid="agent-personas-enabled"
        checked={enabled}
        onCheckedChange={onEnabledChange}
        disabled={pending}
        aria-labelledby="agent-personas-enabled-label"
        aria-describedby="agent-personas-enabled-desc"
        className="settings-toggle data-[state=checked]:bg-[var(--accent-primary)]"
      />
    </div>
  );
}
