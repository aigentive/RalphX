import { useEffect, useState } from "react";
import { BriefcaseBusiness } from "lucide-react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { ProjectSettings } from "@/types/settings";

import { SectionCard } from "../SettingsView.shared";
import ExecutionSection from "./ExecutionSection";
import WorkspaceReviewSection from "./WorkspaceReviewSection";

export type WorkspaceSettingsTab = "general" | "review";

export default function WorkspaceSettingsSection({
  settings,
  disabled,
  onSettingsChange,
  initialTab = "general",
}: {
  settings: ProjectSettings;
  disabled: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
  initialTab?: WorkspaceSettingsTab;
}) {
  const [tab, setTab] = useState<WorkspaceSettingsTab>(initialTab);
  useEffect(() => setTab(initialTab), [initialTab]);
  return (
    <SectionCard
      icon={<BriefcaseBusiness className="h-5 w-5" />}
      title="Workspace"
      description="Configure workspace publishing defaults and review behavior."
    >
      <Tabs value={tab} onValueChange={(value) => setTab(value as WorkspaceSettingsTab)}>
        <TabsList aria-label="Workspace settings">
          <TabsTrigger value="general">General</TabsTrigger>
          <TabsTrigger value="review">Review</TabsTrigger>
        </TabsList>
        <TabsContent value="general" className="mt-4">
          <ExecutionSection
            settings={settings.execution}
            onChange={(changes) =>
              onSettingsChange({
                ...settings,
                execution: { ...settings.execution, ...changes },
              })
            }
            disabled={disabled}
            content="workspace"
            embedded
          />
        </TabsContent>
        <TabsContent value="review" className="mt-4">
          <WorkspaceReviewSection embedded />
        </TabsContent>
      </Tabs>
    </SectionCard>
  );
}
