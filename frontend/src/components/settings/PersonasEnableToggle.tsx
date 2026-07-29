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
      description="Turn on persona selection for new Project Agent conversations. Experimental."
      checked={enabled}
      disabled={pending}
      onChange={onEnabledChange}
    />
  );
}
