import { useState, type ComponentProps } from "react";
import { toast } from "sonner";

import { useUiStore } from "@/stores/uiStore";
import { isPersonaUnavailableError } from "@/lib/personaErrors";

import { getAgentQueueHaltState } from "./agentExecutionPause";
import { AgentsStartComposer } from "./AgentsStartComposer";
import { parseMcpSetupPreflightFailure } from "./agentStartErrors";

type StartComposerProps = ComponentProps<typeof AgentsStartComposer>;
type StartConversationInput = Parameters<StartComposerProps["onSubmit"]>[0];

interface AgentsStartConversationPanelProps {
  defaultProjectId: StartComposerProps["defaultProjectId"];
  defaultRuntime: StartComposerProps["defaultRuntime"];
  isLoadingProjects: StartComposerProps["isLoadingProjects"];
  modelRegistry: StartComposerProps["modelRegistry"];
  onRuntimePreferenceChange?: StartComposerProps["onRuntimePreferenceChange"];
  onStartAgentConversation: (input: StartConversationInput) => Promise<void>;
  projects: StartComposerProps["projects"];
}

export function AgentsStartConversationPanel({
  defaultProjectId,
  defaultRuntime,
  isLoadingProjects,
  modelRegistry,
  onRuntimePreferenceChange,
  onStartAgentConversation,
  projects,
}: AgentsStartConversationPanelProps) {
  const [isStartingConversation, setIsStartingConversation] = useState(false);
  const executionHaltState = useUiStore((s) =>
    getAgentQueueHaltState({
      ...s.executionStatus,
      isKnown: s.executionStatusKnown,
    })
  );

  return (
    <div className="flex-1 min-w-0 h-full">
      <AgentsStartComposer
        projects={projects}
        defaultProjectId={defaultProjectId}
        defaultRuntime={defaultRuntime}
        executionHaltState={executionHaltState}
        isLoadingProjects={isLoadingProjects}
        isSubmitting={isStartingConversation}
        modelRegistry={modelRegistry}
        {...(onRuntimePreferenceChange ? { onRuntimePreferenceChange } : {})}
        onSubmit={async (input) => {
          try {
            setIsStartingConversation(true);
            await onStartAgentConversation(input);
          } catch (err) {
            const message =
              err instanceof Error
                ? err.message
                : "Failed to start agent conversation";
            if (
              !isPersonaUnavailableError(message) &&
              !parseMcpSetupPreflightFailure(err)
            ) {
              toast.error(message);
            }
            throw err;
          } finally {
            setIsStartingConversation(false);
          }
        }}
      />
    </div>
  );
}
