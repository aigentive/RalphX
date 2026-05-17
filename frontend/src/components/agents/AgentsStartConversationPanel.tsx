import { useState, type ComponentProps } from "react";
import { toast } from "sonner";

import { normalizeRuntimeSelection } from "./agentOptions";
import { AgentsStartComposer } from "./AgentsStartComposer";

type StartComposerProps = ComponentProps<typeof AgentsStartComposer>;
type StartConversationInput = Parameters<StartComposerProps["onSubmit"]>[0];

interface AgentsStartConversationPanelProps {
  defaultProjectId: StartComposerProps["defaultProjectId"];
  defaultRuntime: StartComposerProps["defaultRuntime"];
  isLoadingProjects: StartComposerProps["isLoadingProjects"];
  onCreateProject: StartComposerProps["onCreateProject"];
  onRuntimePreferenceChange?: StartComposerProps["onRuntimePreferenceChange"];
  onStartAgentConversation: (input: StartConversationInput) => Promise<void>;
  projects: StartComposerProps["projects"];
}

export function AgentsStartConversationPanel({
  defaultProjectId,
  defaultRuntime,
  isLoadingProjects,
  onCreateProject,
  onRuntimePreferenceChange,
  onStartAgentConversation,
  projects,
}: AgentsStartConversationPanelProps) {
  const [isStartingConversation, setIsStartingConversation] = useState(false);

  return (
    <div className="flex-1 min-w-0 h-full">
      <AgentsStartComposer
        projects={projects}
        defaultProjectId={defaultProjectId}
        defaultRuntime={normalizeRuntimeSelection(defaultRuntime)}
        isLoadingProjects={isLoadingProjects}
        isSubmitting={isStartingConversation}
        onCreateProject={onCreateProject}
        {...(onRuntimePreferenceChange ? { onRuntimePreferenceChange } : {})}
        onSubmit={async (input) => {
          try {
            setIsStartingConversation(true);
            await onStartAgentConversation(input);
          } catch (err) {
            toast.error(
              err instanceof Error
                ? err.message
                : "Failed to start agent conversation",
            );
            throw err;
          } finally {
            setIsStartingConversation(false);
          }
        }}
      />
    </div>
  );
}
