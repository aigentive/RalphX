import { useEffect, useState } from "react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

import {
  IdeationSettingsContent,
  type IdeationSettingsController,
} from "../IdeationSettingsPanel";
import { SettingsSection } from "../SettingsView.shared";
import AutonomyPolicySection from "./AutonomyPolicySection";
import ReviewPolicySection from "./ReviewPolicySection";

export type TasksSettingsTab = "general" | "review-policy" | "autonomy-policy";

export default function TasksSettingsSection({
  controller,
  initialTab = "general",
}: {
  controller: IdeationSettingsController;
  initialTab?: TasksSettingsTab;
}) {
  const [tab, setTab] = useState<TasksSettingsTab>(initialTab);
  useEffect(() => setTab(initialTab), [initialTab]);

  return (
    <SettingsSection>
      <Tabs value={tab} onValueChange={(value) => setTab(value as TasksSettingsTab)}>
        <TabsList aria-label="Tasks settings">
          <TabsTrigger value="general">General</TabsTrigger>
          <TabsTrigger value="review-policy">Review Policy</TabsTrigger>
          <TabsTrigger value="autonomy-policy">Autonomy Policy</TabsTrigger>
        </TabsList>
        <TabsContent value="general" className="mt-4">
          <IdeationSettingsContent controller={controller} surface="tasks" embedded />
        </TabsContent>
        <TabsContent value="review-policy" className="mt-4">
          <ReviewPolicySection embedded />
        </TabsContent>
        <TabsContent value="autonomy-policy" className="mt-4">
          <AutonomyPolicySection embedded />
        </TabsContent>
      </Tabs>
    </SettingsSection>
  );
}
