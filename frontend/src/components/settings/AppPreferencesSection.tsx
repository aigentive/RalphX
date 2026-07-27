import { BookOpenCheck } from "lucide-react";
import { useCallback } from "react";

import { SectionCard, ToggleSettingRow } from "./SettingsView.shared";
import { useSkillsEnabled } from "@/stores/skillsSettingsStore";
import { useUiStore } from "@/stores/uiStore";
import { DEFAULT_APP_VIEW } from "@/types/app-view";

export function AppPreferencesSection() {
  const [skillsEnabled, setSkillsEnabled] = useSkillsEnabled();
  const currentView = useUiStore((s) => s.currentView);
  const setCurrentView = useUiStore((s) => s.setCurrentView);

  const handleSkillsEnabledChange = useCallback(
    (enabled: boolean) => {
      setSkillsEnabled(enabled);
      if (!enabled && currentView === "skills") {
        setCurrentView(DEFAULT_APP_VIEW);
      }
    },
    [currentView, setCurrentView, setSkillsEnabled],
  );

  return (
    <SectionCard
      icon={<BookOpenCheck className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />}
      title="App Preferences"
      description="Workspace-level product surfaces shown in the main application shell"
    >
      <ToggleSettingRow
        id="skills-surface-enabled"
        label="Skills"
        description="Show learned-skill navigation and conversation skill tools"
        checked={skillsEnabled}
        disabled={false}
        onChange={handleSkillsEnabledChange}
      />
    </SectionCard>
  );
}
