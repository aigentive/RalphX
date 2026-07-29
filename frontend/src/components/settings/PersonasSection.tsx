import { useFeatureFlags, useUpdateFeatureFlags } from "@/hooks/useFeatureFlags";

import { PersonasEnableToggle } from "./PersonasEnableToggle";
import { PersonasManagementSection } from "./PersonasManagementSection";

export function PersonasSection() {
  const { data: featureFlags } = useFeatureFlags();
  const updateFeatureFlags = useUpdateFeatureFlags();
  const agentPersonasEnabled = featureFlags.agentPersonas ?? false;

  return (
    <section aria-label="Personas" className="space-y-6">
      <PersonasEnableToggle
        enabled={agentPersonasEnabled}
        pending={updateFeatureFlags.isPending}
        onEnabledChange={(agentPersonas) => updateFeatureFlags.mutate({ agentPersonas })}
      />
      {agentPersonasEnabled && (
        <PersonasManagementSection
          standaloneConversations={featureFlags.standaloneConversations ?? false}
        />
      )}
    </section>
  );
}
