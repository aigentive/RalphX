import type { ReactNode } from "react";

import { AgentsShellLayout } from "./AgentsShellLayout";
import { AgentsConversationMainRegion } from "./AgentsConversationMainRegion";
import { AgentsConversationSideRegions } from "./AgentsConversationSideRegions";
import { useAgentsViewController } from "./useAgentsViewController";

interface AgentsViewProps {
  footer?: ReactNode;
  projectId: string;
  onCreateProject: () => void;
  onOpenAutomation?: (automationId: string) => void;
}

export function AgentsView({
  footer,
  projectId,
  onCreateProject,
  onOpenAutomation,
}: AgentsViewProps) {
  const {
    mainRegionProps,
    shellProps,
    sideRegionProps,
  } = useAgentsViewController({
    projectId,
    onCreateProject,
    ...(onOpenAutomation ? { onOpenAutomation } : {}),
  });

  return (
    <AgentsShellLayout {...shellProps} {...(footer !== undefined ? { footer } : {})}>
      <AgentsConversationMainRegion {...mainRegionProps} />
      <AgentsConversationSideRegions {...sideRegionProps} />
    </AgentsShellLayout>
  );
}
