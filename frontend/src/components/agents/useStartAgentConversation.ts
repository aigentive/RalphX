import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { InfiniteData, QueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

import {
  chatApi,
  type AgentConversationBaseSelection,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type ComposerArtifactReference,
  type ComposerIntegrationReference,
  type ComposerProjectReference,
  type ChatMessageResponse,
  type ConversationMessagesPageResponse,
  type ConversationTimelinePageResponse,
  type CapabilityIntent,
  type TeamIntent,
} from "@/api/chat";
import { automationsApi } from "@/api/automations";
import type { AutomationAuthoringMode } from "@/api/automations";
import { ticketingApi, type TicketRef } from "@/api/ticketing";
import type { MessageAttachment } from "@/components/Chat/MessageAttachments";
import { serializeComposerReferencesMetadata } from "@/components/Chat/MessageReferences.parse";
import {
  chatKeys,
  createOptimisticConversationId,
  invalidateConversationDataQueries,
} from "@/hooks/useChat";
import { invalidateAutomationQueries } from "@/hooks/useAutomations";
import { useAgentModels } from "@/hooks/useAgentModels";
import { getModelLabel } from "@/lib/model-utils";
import { extractErrorMessage } from "@/lib/errors";
import {
  useAgentSessionStore,
  type AgentArtifactState,
  type AgentArtifactTab,
  type AgentRuntimeProviderContext,
  type AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { invalidateAgentSidebarConversations } from "@/hooks/agentSidebarConversationKeys";
import { conversationFolderReferencesApi } from "@/api/conversation-folder-references";
import type { ChatConversation } from "@/types/chat-conversation";

import { DEFAULT_AGENT_ARTIFACT_UI_STATE } from "./agentArtifactUiStore";
import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import {
  hasJiraIntegrationReference,
  invalidateAgentConversationJiraIssue,
} from "./agentJiraIssueQueries";
import {
  hasLinearIntegrationReference,
  invalidateAgentConversationLinearIssue,
} from "./agentLinearIssueQueries";
import {
  hasGranolaIntegrationReference,
  invalidateAgentConversationGranolaNote,
} from "./agentGranolaNoteQueries";
import {
  hasClickUpIntegrationReference,
  invalidateAgentConversationClickUpTicket,
} from "./agentClickUpTicketQueries";
import {
  buildAgentStartConversationRetryInput,
  parseLinkedSetupFailure,
  parseMcpSetupPreflightFailure,
} from "./agentStartErrors";
import {
  normalizeRuntimeForPersistence,
  normalizeRuntimeSelection,
} from "./agentOptions";
import { uploadDraftAttachment } from "./chatAttachmentUpload";

type SupportedStartIntegrationTab = Extract<
  AgentArtifactTab,
  "jira" | "linear" | "clickup" | "granola"
>;

interface HandleAutoManagedTitleArgs {
  content: string;
  conversationId: string;
  targetProjectId: string | null;
  shouldSpawnSessionNamer: boolean;
  providerHarness?: string | null;
}

interface UseStartAgentConversationArgs {
  handleAutoManagedTitle: (args: HandleAutoManagedTitleArgs) => void;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  queryClient: QueryClient;
  selectConversation: (
    projectId: string | null,
    conversationId: string,
  ) => void;
  setActiveConversation: (
    contextKey: string,
    conversationId: string | null,
  ) => void;
  setFocusedProject: (projectId: string | null) => void;
  setOptimisticConversationsById: Dispatch<
    SetStateAction<Record<string, AgentConversation>>
  >;
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
  setOptimisticWorkspacesByConversationId: Dispatch<
    SetStateAction<Record<string, AgentConversationWorkspace>>
  >;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string | null,
    runtime: AgentRuntimeSelection,
  ) => void;
  onJiraLinked?: (conversationId: string) => void;
  onLinearLinked?: (conversationId: string) => void;
  onClickUpLinked?: (conversationId: string) => void;
  onGranolaLinked?: (conversationId: string) => void;
}

const AUTOMATION_DRAFT_TITLE_MAX_LENGTH = 80;
const SEEDED_AGENT_CONVERSATION_ALREADY_STARTED =
  "SEEDED_AGENT_CONVERSATION_ALREADY_STARTED";

function automationDraftNameFromContent(content: string): string | undefined {
  const normalized = content.replace(/\s+/g, " ").trim();
  if (!normalized) {
    return undefined;
  }
  const withoutTrailingPunctuation = normalized.replace(/[.!?;:,\s]+$/u, "");
  const title =
    withoutTrailingPunctuation.length > 0
      ? withoutTrailingPunctuation
      : normalized;
  if (title.length <= AUTOMATION_DRAFT_TITLE_MAX_LENGTH) {
    return title;
  }
  return `${title.slice(0, AUTOMATION_DRAFT_TITLE_MAX_LENGTH - 3).trimEnd()}...`;
}

export function useStartAgentConversation({
  handleAutoManagedTitle,
  invalidateProjectConversations,
  queryClient,
  selectConversation,
  setActiveConversation,
  setFocusedProject,
  setOptimisticConversationsById,
  setOptimisticSelectedConversationId,
  setOptimisticWorkspacesByConversationId,
  setRuntimeForConversation,
  onJiraLinked,
  onLinearLinked,
  onClickUpLinked,
  onGranolaLinked,
}: UseStartAgentConversationArgs) {
  const { registry: modelRegistry } = useAgentModels();
  const queueMessage = useChatStore((s) => s.queueMessage);
  const setAgentActivityLabel = useChatStore((s) => s.setAgentActivityLabel);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);
  const setEffectiveModel = useChatStore((s) => s.setEffectiveModel);
  const handleStartAgentConversation = useCallback(
    async ({
      projectId: targetProjectId,
      content,
      runtime,
      runtimeProviderContext,
      useRoleDefault = false,
      mode,
      automationAuthoringMode,
      base,
      files,
      folders = [],
      codexFastMode,
      personaId,
      capabilityIntent,
      teamIntent,
      sourcePersonaId,
      composerArtifactReferences,
      composerIntegrationReferences,
      composerProjectReferences,
    }: {
      projectId: string | null;
      content: string;
      runtime: AgentRuntimeSelection;
      runtimeProviderContext?: AgentRuntimeProviderContext | undefined;
      useRoleDefault?: boolean | undefined;
      mode: AgentConversationWorkspaceMode;
      automationAuthoringMode?: AutomationAuthoringMode | undefined;
      base: AgentConversationBaseSelection | null;
      files: File[];
      folders?: { folderPath: string; displayName: string }[];
      codexFastMode?: boolean | null;
      personaId?: string | null;
      capabilityIntent?: CapabilityIntent | null;
      teamIntent?: TeamIntent | null;
      sourcePersonaId?: string | undefined;
      composerArtifactReferences?: ComposerArtifactReference[] | undefined;
      composerIntegrationReferences?:
        ComposerIntegrationReference[] | undefined;
      composerProjectReferences?: ComposerProjectReference[] | undefined;
    }) => {
      const isStandalone = targetProjectId === null;
      const conversationContextType = isStandalone ? "standalone" : "project";
      const effectiveMode =
        isStandalone && mode !== "persona_builder" ? "chat" : mode;
      const effectiveFolders =
        isStandalone && mode !== "persona_builder" ? [] : folders;
      const effectiveProjectReferences = isStandalone
        ? undefined
        : composerProjectReferences;
      const persistenceRuntime = normalizeRuntimeForPersistence(
        runtime,
        modelRegistry,
      );
      const normalizedRuntime = normalizeRuntimeSelection(
        persistenceRuntime,
        modelRegistry,
        runtimeProviderContext?.supportedEfforts,
        runtimeProviderContext?.supportedModelAliases,
      );
      const startIntegrationTab = getSupportedStartIntegrationTab(
        composerIntegrationReferences,
      );
      const startArtifactState = buildStartArtifactState(startIntegrationTab);
      const seedConversationState = (
        conversation: ChatConversation,
        workspace: AgentConversationWorkspace | null | undefined,
        optimisticMessages: ChatMessageResponse[] = [],
      ): string => {
        const conversationId = conversation.id;
        const optimisticConversation = toProjectAgentConversation(conversation);
        const storeKey = getAgentConversationStoreKey(optimisticConversation);

        setOptimisticConversationsById((current) => ({
          ...current,
          [conversationId]: optimisticConversation,
        }));
        if (workspace) {
          setOptimisticWorkspacesByConversationId((current) => ({
            ...current,
            [conversationId]: workspace,
          }));
        }
        queryClient.setQueryData(chatKeys.conversation(conversationId), {
          conversation,
          messages: optimisticMessages,
        });
        queryClient.setQueryData(
          chatKeys.conversationSummary(conversationId),
          conversation,
        );
        if (optimisticMessages.length > 0) {
          queryClient.setQueryData<
            InfiniteData<ConversationMessagesPageResponse>
          >(chatKeys.conversationHistory(conversationId), {
            pages: [
              {
                conversation,
                messages: optimisticMessages,
                limit: 40,
                offset: 0,
                totalMessageCount: optimisticMessages.length,
                hasOlder: false,
              },
            ],
            pageParams: [0],
          });
          queryClient.setQueryData<
            InfiniteData<ConversationTimelinePageResponse>
          >(chatKeys.conversationTimeline(conversationId), {
            pages: [
              {
                conversation,
                items: [],
                messages: optimisticMessages,
                limit: 40,
                beforeSequence: null,
                totalItemCount: optimisticMessages.length,
                hasOlder: false,
                oldestLoadedSequence: null,
                newestLoadedSequence: null,
              },
            ],
            pageParams: [null],
          });
        }
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          workspace ?? null,
        );
        setOptimisticSelectedConversationId(conversationId);
        setFocusedProject(targetProjectId);
        setRuntimeForConversation(
          conversationId,
          targetProjectId,
          normalizedRuntime,
        );
        useAgentSessionStore
          .getState()
          .setArtifactState(conversationId, startArtifactState);
        selectConversation(targetProjectId, conversationId);
        setActiveConversation(storeKey, conversationId);
        return storeKey;
      };
      const removeOptimisticConversation = (
        conversationId: string,
        storeKey: string,
      ) => {
        setOptimisticConversationsById((current) => {
          if (!(conversationId in current)) return current;
          const next = { ...current };
          delete next[conversationId];
          return next;
        });
        setOptimisticWorkspacesByConversationId((current) => {
          if (!(conversationId in current)) return current;
          const next = { ...current };
          delete next[conversationId];
          return next;
        });
        setOptimisticSelectedConversationId((current) =>
          current === conversationId ? null : current,
        );
        if (
          useAgentSessionStore.getState().selectedConversationId ===
          conversationId
        ) {
          useAgentSessionStore.getState().clearSelection();
        }
        queryClient.removeQueries({
          queryKey: chatKeys.conversation(conversationId),
        });
        queryClient.removeQueries({
          queryKey: chatKeys.conversationSummary(conversationId),
        });
        queryClient.removeQueries({
          queryKey: chatKeys.conversationHistory(conversationId),
        });
        queryClient.removeQueries({
          queryKey: chatKeys.conversationTimeline(conversationId),
        });
        queryClient.removeQueries({
          queryKey: ["agents", "conversation-workspace", conversationId],
        });
        setActiveConversation(storeKey, null);
        setAgentActivityLabel(storeKey, null);
        setAgentRunning(storeKey, false);
        setSending(storeKey, false);
      };

      const now = new Date().toISOString();
      const optimisticReferenceMetadata = serializeComposerReferencesMetadata({
        folderReferences: effectiveFolders,
        projectReferences: effectiveProjectReferences,
        integrationReferences: composerIntegrationReferences,
        artifactReferences: composerArtifactReferences,
      });
      const optimisticCoordinationMode =
        capabilityIntent?.coordinationMode ??
        teamIntent?.coordinationMode ??
        "solo";
      const optimisticConversationId = createOptimisticConversationId();
      const initialConversation: ChatConversation = {
        id: optimisticConversationId,
        contextType: conversationContextType,
        contextId: targetProjectId ?? optimisticConversationId,
        claudeSessionId: null,
        providerSessionId: null,
        providerHarness: normalizedRuntime.provider,
        coordinationMode: optimisticCoordinationMode,
        upstreamProvider: null,
        providerProfile: null,
        agentMode: effectiveMode,
        title: null,
        messageCount: 1,
        lastMessageAt: now,
        createdAt: now,
        updatedAt: now,
        archivedAt: null,
      };
      const optimisticAttachments = buildOptimisticMessageAttachments(files);
      const optimisticUserMessage = buildOptimisticUserMessage({
        conversation: initialConversation,
        content,
        runtime: normalizedRuntime,
        ...(optimisticReferenceMetadata
          ? { metadata: optimisticReferenceMetadata }
          : {}),
        ...(optimisticAttachments
          ? { attachments: optimisticAttachments }
          : {}),
      });
      const optimisticStoreKey = seedConversationState(
        initialConversation,
        null,
        [optimisticUserMessage],
      );
      setAgentActivityLabel(optimisticStoreKey, "Creating chat");
      setEffectiveModel(optimisticStoreKey, {
        id: normalizedRuntime.modelId,
        label: getModelLabel(normalizedRuntime.modelId),
      });
      setAgentRunning(optimisticStoreKey, true);
      setSending(optimisticStoreKey, true);

      const requiresSeededConversation =
        isStandalone ||
        effectiveMode === "automation" ||
        files.length > 0 ||
        effectiveFolders.length > 0;
      let seededStoreKey: string | null = null;
      let seededConversationId: string | null = null;
      let abortableSeededConversationId: string | null = null;
      try {
        const automationDraft =
          effectiveMode === "automation"
            ? await automationsApi.createDraft({
                projectId: targetProjectId!,
                name: automationDraftNameFromContent(content),
                ...(base ? { base } : {}),
                ...(automationAuthoringMode
                  ? { authoringMode: automationAuthoringMode }
                  : {}),
              })
            : null;
        const setupConversationId =
          automationDraft?.setupConversationId ?? null;
        if (effectiveMode === "automation" && !setupConversationId) {
          throw new Error(
            "Automation draft did not create a setup conversation",
          );
        }
        if (automationDraft) {
          await automationsApi.setupAgent.updateAutomation(
            setupConversationId!,
            {
              providerHarness: normalizedRuntime.provider,
              modelId: normalizedRuntime.modelId,
              logicalEffort: normalizedRuntime.effort,
            },
          );
          invalidateAutomationQueries(
            queryClient,
            automationDraft.automation.id,
          );
        }

        const resultConversationSeed =
          requiresSeededConversation && effectiveMode !== "automation"
            ? effectiveMode === "persona_builder"
              ? await chatApi.createConversation(
                  conversationContextType,
                  targetProjectId,
                  undefined,
                  effectiveMode,
                )
              : await chatApi.createConversation(
                  conversationContextType,
                  targetProjectId,
                )
            : null;
        const seededConversation: ChatConversation | null =
          resultConversationSeed
            ? {
                ...resultConversationSeed,
                agentMode: effectiveMode,
                coordinationMode: optimisticCoordinationMode,
              }
            : automationDraft
              ? {
                  id: setupConversationId!,
                  contextType: "project",
                  contextId: targetProjectId!,
                  claudeSessionId: null,
                  providerSessionId: null,
                  providerHarness: normalizedRuntime.provider,
                  upstreamProvider: null,
                  providerProfile: null,
                  agentMode: "automation",
                  automationId: automationDraft.automation.id,
                  automationRunId: null,
                  parentConversationId: null,
                  coordinationMode: optimisticCoordinationMode,
                  title: automationDraft.automation.name,
                  messageCount: 1,
                  lastMessageAt: now,
                  createdAt: now,
                  updatedAt: now,
                  archivedAt: null,
                }
              : null;
        abortableSeededConversationId = resultConversationSeed?.id ?? null;
        const activeConversation = seededConversation ?? initialConversation;
        const storeKey = seededConversation
          ? getAgentConversationStoreKey({
              id: seededConversation.id,
              contextType: seededConversation.contextType,
              contextId: seededConversation.contextId,
            })
          : optimisticStoreKey;
        const activeOptimisticUserMessage = seededConversation
          ? buildOptimisticUserMessage({
              conversation: seededConversation,
              content,
              runtime: normalizedRuntime,
              ...(optimisticReferenceMetadata
                ? { metadata: optimisticReferenceMetadata }
                : {}),
              ...(optimisticAttachments
                ? { attachments: optimisticAttachments }
                : {}),
            })
          : optimisticUserMessage;
        if (seededConversation) {
          seededStoreKey = storeKey;
          seededConversationId = seededConversation.id;
          seedConversationState(seededConversation, null, [
            activeOptimisticUserMessage,
          ]);
          removeOptimisticConversation(
            initialConversation.id,
            optimisticStoreKey,
          );
          setEffectiveModel(storeKey, {
            id: normalizedRuntime.modelId,
            label: getModelLabel(normalizedRuntime.modelId),
          });
          setAgentActivityLabel(
            storeKey,
            files.length > 0 ? "Uploading files" : "Setup workspace",
          );
          setAgentRunning(storeKey, true);
          setSending(storeKey, true);
        } else {
          setAgentActivityLabel(storeKey, "Setup workspace");
        }

        let uploadedAttachmentIds: string[] = [];
        if (effectiveFolders.length > 0) {
          if (!seededConversation) {
            throw new Error(
              "Folder registration requires a draft conversation",
            );
          }
          await Promise.all(
            effectiveFolders.map((folder) =>
              conversationFolderReferencesApi.add({
                conversationId: seededConversation.id,
                folderPath: folder.folderPath,
                displayName: folder.displayName,
              }),
            ),
          );
        }
        if (files.length > 0) {
          if (!seededConversation) {
            throw new Error("Attachment upload requires a draft conversation");
          }
          const uploadedAttachments = await Promise.all(
            files.map((file) =>
              uploadDraftAttachment(seededConversation.id, file),
            ),
          );
          uploadedAttachmentIds = uploadedAttachments.map(
            (attachment) => attachment.id,
          );
          setAgentActivityLabel(storeKey, "Setup workspace");
        }

        const startInput = {
          ...(targetProjectId ? { projectId: targetProjectId } : {}),
          content,
          ...(seededConversation
            ? { conversationId: seededConversation.id }
            : {}),
          ...(!useRoleDefault
            ? {
                providerHarness: normalizedRuntime.provider,
                modelId: normalizedRuntime.modelId,
                logicalEffort: normalizedRuntime.effort,
                ...(codexFastMode !== undefined
                  ? {
                      codexFastMode:
                        normalizedRuntime.provider === "codex"
                          ? codexFastMode
                          : null,
                    }
                  : {}),
                ...(!isStandalone &&
                effectiveMode !== "persona_builder" &&
                personaId
                  ? { personaId }
                  : {}),
                ...(!isStandalone &&
                effectiveMode !== "persona_builder" &&
                capabilityIntent
                  ? { capabilityIntent }
                  : {}),
                ...(!isStandalone &&
                effectiveMode !== "persona_builder" &&
                teamIntent
                  ? { teamIntent }
                  : {}),
              }
            : {}),
          mode: effectiveMode,
          ...(effectiveMode === "persona_builder" && sourcePersonaId
            ? { sourcePersonaId }
            : {}),
          ...(effectiveProjectReferences?.length
            ? { composerProjectReferences: effectiveProjectReferences }
            : {}),
          ...(composerIntegrationReferences?.length
            ? { composerIntegrationReferences }
            : {}),
          ...(composerArtifactReferences?.length
            ? { composerArtifactReferences }
            : {}),
          ...(!isStandalone && base ? { base } : {}),
        };
        const ticketRef = ticketRefFromIntegrationReferences(
          composerIntegrationReferences,
        );
        const result =
          ticketRef && targetProjectId
            ? await ticketingApi.startWorkFromTicket({
                ...startInput,
                projectId: targetProjectId,
                ticketRef,
              })
            : await chatApi.startAgentConversation(startInput);
        abortableSeededConversationId = null;
        const resolvedConversation: ChatConversation = {
          ...result.conversation,
          agentMode: result.conversation.agentMode ?? effectiveMode,
        };
        const resolvedConversationId = resolvedConversation.id;
        const optimisticWorkspace = result.workspace;
        const resolvedStoreKey =
          getAgentConversationStoreKey(resolvedConversation);
        const resolvedOptimisticUserMessage =
          resolvedConversationId === activeConversation.id
            ? activeOptimisticUserMessage
            : buildOptimisticUserMessage({
                conversation: resolvedConversation,
                content,
                runtime: normalizedRuntime,
                ...(optimisticReferenceMetadata
                  ? { metadata: optimisticReferenceMetadata }
                  : {}),
                ...(optimisticAttachments
                  ? { attachments: optimisticAttachments }
                  : {}),
              });
        seedConversationState(
          resolvedConversation,
          optimisticWorkspace ?? null,
          [resolvedOptimisticUserMessage],
        );
        if (!seededConversation && resolvedStoreKey !== storeKey) {
          removeOptimisticConversation(
            initialConversation.id,
            optimisticStoreKey,
          );
        }
        if (resolvedStoreKey !== storeKey) {
          setAgentActivityLabel(storeKey, null);
          setAgentRunning(storeKey, false);
          setSending(storeKey, false);
          setEffectiveModel(resolvedStoreKey, {
            id: normalizedRuntime.modelId,
            label: getModelLabel(normalizedRuntime.modelId),
          });
          setAgentRunning(resolvedStoreKey, true);
        }
        setAgentActivityLabel(resolvedStoreKey, "Agent working");
        if (
          result.sendResult.wasQueued &&
          result.sendResult.queuedMessageId != null
        ) {
          queueMessage(
            resolvedStoreKey,
            content,
            result.sendResult.queuedMessageId,
            uploadedAttachmentIds.length > 0
              ? uploadedAttachmentIds
              : undefined,
          );
        }
        if (result.sendResult.wasQueued || result.sendResult.queuedAsPending) {
          setAgentRunning(resolvedStoreKey, false);
        }
        setSending(resolvedStoreKey, false);
        invalidateConversationDataQueries(queryClient, resolvedConversationId);
        if (hasJiraIntegrationReference(composerIntegrationReferences)) {
          if (startIntegrationTab === "jira") {
            onJiraLinked?.(resolvedConversationId);
          }
          await invalidateAgentConversationJiraIssue(
            queryClient,
            resolvedConversationId,
          );
        }
        if (hasLinearIntegrationReference(composerIntegrationReferences)) {
          if (startIntegrationTab === "linear") {
            onLinearLinked?.(resolvedConversationId);
          }
          await invalidateAgentConversationLinearIssue(
            queryClient,
            resolvedConversationId,
          );
        }
        if (hasClickUpIntegrationReference(composerIntegrationReferences)) {
          if (startIntegrationTab === "clickup") {
            onClickUpLinked?.(resolvedConversationId);
          }
          await invalidateAgentConversationClickUpTicket(
            queryClient,
            resolvedConversationId,
          );
        }
        if (hasGranolaIntegrationReference(composerIntegrationReferences)) {
          if (startIntegrationTab === "granola") {
            onGranolaLinked?.(resolvedConversationId);
          }
          await invalidateAgentConversationGranolaNote(
            queryClient,
            resolvedConversationId,
          );
        }
        if (targetProjectId) {
          await invalidateProjectConversations(targetProjectId);
        } else {
          await invalidateAgentSidebarConversations(queryClient);
        }
        handleAutoManagedTitle({
          content,
          conversationId: resolvedConversationId,
          targetProjectId,
          shouldSpawnSessionNamer: true,
          providerHarness: normalizedRuntime.provider,
        });
      } catch (err) {
        let seededAbortRefused = false;
        if (abortableSeededConversationId) {
          try {
            await invoke("abort_seeded_agent_conversation", {
              conversationId: abortableSeededConversationId,
            });
          } catch (abortError) {
            seededAbortRefused = extractErrorMessage(abortError, "").includes(
              SEEDED_AGENT_CONVERSATION_ALREADY_STARTED,
            );
            if (!seededAbortRefused) {
              console.warn(
                "Failed to abort a never-started seeded agent conversation",
                abortError,
              );
            }
          }
        }
        const linkedFailure = parseLinkedSetupFailure(err);
        const mcpFailure = parseMcpSetupPreflightFailure(err);
        if (linkedFailure) {
          useAgentSessionStore.getState().setStartConversationFailure({
            kind: "linked_setup",
            message: linkedFailure.message,
            retryInput: buildAgentStartConversationRetryInput({
              projectId: targetProjectId,
              content,
              runtime: normalizedRuntime,
              runtimeProviderContext,
              useRoleDefault,
              mode: effectiveMode,
              base,
              codexFastMode,
              personaId,
              capabilityIntent,
              teamIntent,
              composerArtifactReferences,
              composerIntegrationReferences,
              composerProjectReferences,
            }),
          });
        } else if (mcpFailure) {
          useAgentSessionStore.getState().setStartConversationFailure({
            kind: "mcp_setup",
            ...mcpFailure,
            retryInput: buildAgentStartConversationRetryInput({
              projectId: targetProjectId,
              content,
              runtime: normalizedRuntime,
              runtimeProviderContext,
              useRoleDefault,
              mode,
              base,
              codexFastMode,
              personaId,
              capabilityIntent,
              teamIntent,
              composerArtifactReferences,
              composerIntegrationReferences,
              composerProjectReferences,
            }),
          });
        }
        if (seededStoreKey) {
          setAgentActivityLabel(seededStoreKey, null);
          setAgentRunning(seededStoreKey, false);
          setSending(seededStoreKey, false);
        }
        if (seededConversationId && seededStoreKey) {
          if (seededAbortRefused) {
            invalidateConversationDataQueries(
              queryClient,
              seededConversationId,
            );
            await invalidateAgentSidebarConversations(queryClient);
            setOptimisticSelectedConversationId(seededConversationId);
            setFocusedProject(targetProjectId);
            selectConversation(targetProjectId, seededConversationId);
            setActiveConversation(seededStoreKey, seededConversationId);
          } else {
            removeOptimisticConversation(seededConversationId, seededStoreKey);
          }
        }
        removeOptimisticConversation(
          initialConversation.id,
          optimisticStoreKey,
        );
        if (!seededAbortRefused) {
          setOptimisticSelectedConversationId(null);
          useAgentSessionStore.getState().clearSelection();
        }
        throw err;
      }
    },
    [
      handleAutoManagedTitle,
      invalidateProjectConversations,
      modelRegistry,
      queryClient,
      queueMessage,
      selectConversation,
      setActiveConversation,
      setAgentActivityLabel,
      setAgentRunning,
      setEffectiveModel,
      setFocusedProject,
      setOptimisticConversationsById,
      setOptimisticSelectedConversationId,
      setOptimisticWorkspacesByConversationId,
      setRuntimeForConversation,
      onJiraLinked,
      onLinearLinked,
      onClickUpLinked,
      onGranolaLinked,
      setSending,
    ],
  );

  return handleStartAgentConversation;
}

function ticketRefFromIntegrationReferences(
  references: ComposerIntegrationReference[] | undefined,
): TicketRef | null {
  for (const reference of references ?? []) {
    const provider = reference.provider.trim().toLowerCase();
    const kind = reference.kind.trim().toLowerCase();
    const ticketProvider =
      kind === "jira" && (provider === "atlassian" || provider === "jira")
        ? "jira"
        : kind === "linear" && provider === "linear"
          ? "linear"
          : kind === "clickup" && provider === "clickup"
            ? "clickup"
            : null;
    if (ticketProvider && reference.id.trim()) {
      return {
        provider: ticketProvider,
        id: reference.id.trim(),
        ...(reference.key?.trim() ? { key: reference.key.trim() } : {}),
      };
    }
  }
  return null;
}

function getSupportedStartIntegrationTab(
  references: readonly ComposerIntegrationReference[] | null | undefined,
): SupportedStartIntegrationTab | null {
  for (const reference of references ?? []) {
    if (reference.kind === "jira") {
      return "jira";
    }
    if (reference.kind === "linear") {
      return "linear";
    }
    if (reference.provider === "clickup" && reference.kind === "clickup") {
      return "clickup";
    }
    if (reference.provider === "granola" && reference.kind === "note") {
      return "granola";
    }
  }
  return null;
}

function buildStartArtifactState(
  integrationTab: SupportedStartIntegrationTab | null,
): AgentArtifactState {
  return {
    isOpen: integrationTab !== null,
    activeTab: integrationTab ?? "plan",
    taskMode: DEFAULT_AGENT_ARTIFACT_UI_STATE.taskMode,
    hiddenTabs: [],
  };
}

function buildOptimisticUserMessage({
  conversation,
  content,
  runtime,
  metadata,
  attachments,
}: {
  conversation: ChatConversation;
  content: string;
  runtime: AgentRuntimeSelection;
  metadata?: string | null;
  attachments?: MessageAttachment[];
}): ChatMessageResponse {
  return {
    id: `optimistic:${conversation.id}:initial-user`,
    sessionId: null,
    projectId:
      conversation.contextType === "project" ? conversation.contextId : null,
    taskId: null,
    role: "user",
    content,
    metadata: metadata ?? null,
    parentMessageId: null,
    conversationId: conversation.id,
    toolCalls: null,
    contentBlocks: null,
    ...(attachments && attachments.length > 0 ? { attachments } : {}),
    sender: null,
    attributionSource: null,
    providerHarness: runtime.provider,
    providerSessionId: null,
    upstreamProvider: null,
    providerProfile: null,
    logicalModel: runtime.modelId,
    effectiveModelId: runtime.modelId,
    logicalEffort: runtime.effort,
    effectiveEffort: null,
    inputTokens: null,
    outputTokens: null,
    cacheCreationTokens: null,
    cacheReadTokens: null,
    estimatedUsd: null,
    createdAt: new Date().toISOString(),
  };
}

function buildOptimisticMessageAttachments(
  files: File[],
): MessageAttachment[] | undefined {
  if (files.length === 0) {
    return undefined;
  }

  return files.map((file, index) => ({
    id: `optimistic:${index}:${file.name}`,
    fileName: file.name || "attachment",
    fileSize: file.size,
    ...(file.type ? { mimeType: file.type } : {}),
    ...(file.type.startsWith("image/") &&
    typeof URL.createObjectURL === "function"
      ? { previewUrl: URL.createObjectURL(file) }
      : {}),
  }));
}
