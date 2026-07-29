import { Drama } from "lucide-react";

import { Separator } from "@/components/ui/separator";
import { useFeatureFlags, useUpdateFeatureFlags } from "@/hooks/useFeatureFlags";

import { PersonasEnableToggle } from "./PersonasEnableToggle";
import { PersonasManagementSection } from "./PersonasManagementSection";
import { SectionCard } from "./SettingsView.shared";

export function PersonasSection() {
  const { data: featureFlags } = useFeatureFlags();
  const updateFeatureFlags = useUpdateFeatureFlags();
  const agentPersonasEnabled = featureFlags.agentPersonas ?? false;

  return (
    <section aria-label="Personas">
      <SectionCard
        icon={<Drama className="h-5 w-5" />}
        title="Agent Personas"
        description="Conversation-bound behavior profiles for Project Agent conversations."
      >
        <PersonasEnableToggle
          enabled={agentPersonasEnabled}
          pending={updateFeatureFlags.isPending}
          onEnabledChange={(agentPersonas) => updateFeatureFlags.mutate({ agentPersonas })}
        />
        {agentPersonasEnabled && (
          <>
            <Separator />
            <PersonasManagementSection
              standaloneConversations={featureFlags.standaloneConversations ?? false}
            />
          </>
        )}
      </SectionCard>
    </section>
  );
}
