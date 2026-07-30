
import {
  IdeationSettingsContent,
  type IdeationSettingsController,
} from "../IdeationSettingsPanel";
import { SettingsSection } from "../SettingsView.shared";

export default function PlanningSettingsSection({
  controller,
}: {
  controller: IdeationSettingsController;
}) {
  return (
    <SettingsSection>
      <IdeationSettingsContent controller={controller} surface="planning" embedded />
    </SettingsSection>
  );
}
