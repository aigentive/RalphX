import type { ReactNode } from "react";

import { AgentsShellLayout } from "./AgentsShellLayout";
import { AgentsConversationMainRegion } from "./AgentsConversationMainRegion";
import { AgentsConversationSideRegions } from "./AgentsConversationSideRegions";
import { useAgentsViewController } from "./useAgentsViewController";

interface AgentsViewProps {
  footer?: ReactNode;
  projectId: string;
  onCreateProject: () => void;
}

export function AgentsView({
  footer,
  projectId,
  onCreateProject,
}: AgentsViewProps) {
  const {
    mainRegionProps,
    shellProps,
    sideRegionProps,
  } = useAgentsViewController({
    projectId,
    onCreateProject,
  });

  return (
    <AgentsShellLayout {...shellProps} {...(footer !== undefined ? { footer } : {})}>
      <AgentsConversationMainRegion {...mainRegionProps} />
      <AgentsConversationSideRegions {...sideRegionProps} />
    </AgentsShellLayout>
  );
}
