import { ToggleSettingRow } from "./SettingsView.shared";

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
    <ToggleSettingRow
      id="agent-personas-enabled"
      label="Enable Agent Personas"
      description="Conversation-bound behavior profiles for Project Agent conversations. Experimental."
      checked={enabled}
      disabled={pending}
      onChange={onEnabledChange}
    />
  );
}
