import { CalendarCheck } from "lucide-react";

import {
  IdeationSettingsContent,
  type IdeationSettingsController,
} from "../IdeationSettingsPanel";
import { SectionCard } from "../SettingsView.shared";

export default function PlanningSettingsSection({
  controller,
}: {
  controller: IdeationSettingsController;
}) {
  return (
    <SectionCard
      icon={<CalendarCheck className="h-5 w-5" />}
      title="Plan verification"
      description="Configure acceptance-triggered automatic plan verification."
    >
      <IdeationSettingsContent controller={controller} surface="planning" embedded />
    </SectionCard>
  );
}
