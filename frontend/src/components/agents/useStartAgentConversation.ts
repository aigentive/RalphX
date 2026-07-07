import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { InfiniteData, QueryClient } from "@tanstack/react-query";

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
} from "@/api/chat";
import { automationsApi } from "@/api/automations";
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
import {
  useAgentSessionStore,
  type AgentArtifactState,
  type AgentArtifactTab,
  type AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
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
import { normalizeRuntimeSelection } from "./agentOptions";
import { uploadDraftAttachment } from "./chatAttachmentUpload";

type SupportedStartIntegrationTab = Extract<
  AgentArtifactTab,
  "jira" | "linear" | "granola"
>;

interface HandleAutoManagedTitleArgs {
  content: string;
  conversationId: string;
  targetProjectId: string;
  shouldSpawnSessionNamer: boolean;
  providerHarness?: string | null;
}

interface UseStartAgentConversationArgs {
  handleAutoManagedTitle: (args: HandleAutoManagedTitleArgs) => void;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  queryClient: QueryClient;
  selectConversation: (projectId: string, conversationId: string) => void;
  setActiveConversation: (contextKey: string, conversationId: string | null) => void;
  setFocusedProject: (projectId: string | null) => void;
  setOptimisticConversationsById: Dispatch<SetStateAction<Record<string, AgentConversation>>>;
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
  setOptimisticWorkspacesByConversationId: Dispatch<
    SetStateAction<Record<string, AgentConversationWorkspace>>
  >;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string,
    runtime: AgentRuntimeSelection
  ) => void;
  onJiraLinked?: (conversationId: string) => void;
  onLinearLinked?: (conversationId: string) => void;
  onGranolaLinked?: (conversationId: string) => void;
}

const AUTOMATION_DRAFT_TITLE_MAX_LENGTH = 80;

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
      mode,
      base,
      files,
      codexFastMode,
      composerArtifactReferences,
      composerIntegrationReferences,
      composerProjectReferences,
    }: {
      projectId: string;
      content: string;
      runtime: AgentRuntimeSelection;
      mode: AgentConversationWorkspaceMode;
      base: AgentConversationBaseSelection | null;
      files: File[];
      codexFastMode?: boolean | null;
      composerArtifactReferences?: ComposerArtifactReference[] | undefined;
      composerIntegrationReferences?: ComposerIntegrationReference[] | undefined;
      composerProjectReferences?: ComposerProjectReference[] | undefined;
    }) => {
      const normalizedRuntime = normalizeRuntimeSelection(runtime, modelRegistry);
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
        if (optimisticMessages.length > 0) {
          queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
            chatKeys.conversationHistory(conversationId),
            {
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
            }
          );
          queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
            chatKeys.conversationTimeline(conversationId),
            {
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
            }
          );
        }
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          workspace ?? null,
        );
        setOptimisticSelectedConversationId(conversationId);
        setFocusedProject(targetProjectId);
        setRuntimeForConversation(conversationId, targetProjectId, normalizedRuntime);
        useAgentSessionStore
          .getState()
          .setArtifactState(conversationId, startArtifactState);
        selectConversation(targetProjectId, conversationId);
        setActiveConversation(storeKey, conversationId);
        return storeKey;
      };
      const removeOptimisticConversation = (conversationId: string, storeKey: string) => {
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
          current === conversationId ? null : current
        );
        queryClient.removeQueries({ queryKey: chatKeys.conversation(conversationId) });
        setActiveConversation(storeKey, null);
        setAgentActivityLabel(storeKey, null);
        setAgentRunning(storeKey, false);
        setSending(storeKey, false);
      };

      const now = new Date().toISOString();
      const optimisticReferenceMetadata = serializeComposerReferencesMetadata({
        projectReferences: composerProjectReferences,
        integrationReferences: composerIntegrationReferences,
        artifactReferences: composerArtifactReferences,
      });
      const initialConversation: ChatConversation = {
        id: createOptimisticConversationId(),
        contextType: "project",
        contextId: targetProjectId,
        claudeSessionId: null,
        providerSessionId: null,
        providerHarness: normalizedRuntime.provider,
        coordinationMode: "solo",
        upstreamProvider: null,
        providerProfile: null,
        agentMode: mode,
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
        ...(optimisticAttachments ? { attachments: optimisticAttachments } : {}),
      });
      const optimisticStoreKey = seedConversationState(initialConversation, null, [
        optimisticUserMessage,
      ]);
      setAgentActivityLabel(optimisticStoreKey, "Creating chat");
      setEffectiveModel(optimisticStoreKey, {
        id: normalizedRuntime.modelId,
        label: getModelLabel(normalizedRuntime.modelId),
      });
      setAgentRunning(optimisticStoreKey, true);
      setSending(optimisticStoreKey, true);

      let seededStoreKey: string | null = null;
      try {
        const automationDraft =
          mode === "automation"
            ? await automationsApi.createDraft({
                projectId: targetProjectId,
                name: automationDraftNameFromContent(content),
              })
            : null;
        const setupConversationId = automationDraft?.setupConversationId ?? null;
        if (mode === "automation" && !setupConversationId) {
          throw new Error("Automation draft did not create a setup conversation");
        }
        if (automationDraft) {
          await automationsApi.setupAgent.updateAutomation(setupConversationId!, {
            providerHarness: normalizedRuntime.provider,
            modelId: normalizedRuntime.modelId,
            logicalEffort: normalizedRuntime.effort,
          });
          invalidateAutomationQueries(queryClient, automationDraft.automation.id);
        }

        const resultConversationSeed =
          mode === "automation"
            ? null
            : await chatApi.createConversation("project", targetProjectId);
        const seededConversation: ChatConversation = resultConversationSeed
          ? {
              ...resultConversationSeed,
              agentMode: mode,
            }
          : {
              id: setupConversationId!,
              contextType: "project",
              contextId: targetProjectId,
              claudeSessionId: null,
              providerSessionId: null,
              providerHarness: normalizedRuntime.provider,
              upstreamProvider: null,
              providerProfile: null,
              agentMode: "automation",
              automationId: automationDraft!.automation.id,
              automationRunId: null,
              parentConversationId: null,
              coordinationMode: "solo",
              title: automationDraft!.automation.name,
              messageCount: 1,
              lastMessageAt: now,
              createdAt: now,
              updatedAt: now,
              archivedAt: null,
            };
        const storeKey = getAgentConversationStoreKey({
          id: seededConversation.id,
          contextType: "project",
          contextId: targetProjectId,
        });
        seededStoreKey = storeKey;
        const seededOptimisticUserMessage = buildOptimisticUserMessage({
          conversation: seededConversation,
          content,
          runtime: normalizedRuntime,
          ...(optimisticReferenceMetadata
            ? { metadata: optimisticReferenceMetadata }
            : {}),
          ...(optimisticAttachments ? { attachments: optimisticAttachments } : {}),
        });
        seedConversationState(seededConversation, null, [seededOptimisticUserMessage]);
        removeOptimisticConversation(initialConversation.id, optimisticStoreKey);
        setEffectiveModel(storeKey, {
          id: normalizedRuntime.modelId,
          label: getModelLabel(normalizedRuntime.modelId),
        });
        setAgentActivityLabel(storeKey, files.length > 0 ? "Uploading files" : "Setup workspace");
        setAgentRunning(storeKey, true);
        setSending(storeKey, true);

        let uploadedAttachmentIds: string[] = [];
        if (files.length > 0) {
          const uploadedAttachments = await Promise.all(
            files.map((file) => uploadDraftAttachment(seededConversation.id, file))
          );
          uploadedAttachmentIds = uploadedAttachments.map((attachment) => attachment.id);
          setAgentActivityLabel(storeKey, "Setup workspace");
        }

        const result = await chatApi.startAgentConversation({
          projectId: targetProjectId,
          content,
          conversationId: seededConversation.id,
          providerHarness: normalizedRuntime.provider,
          modelId: normalizedRuntime.modelId,
          logicalEffort: normalizedRuntime.effort,
          ...(codexFastMode !== undefined
            ? {
                codexFastMode:
                  normalizedRuntime.provider === "codex" ? codexFastMode : null,
              }
            : {}),
          mode,
          ...(composerProjectReferences?.length
            ? { composerProjectReferences }
            : {}),
          ...(composerIntegrationReferences?.length
            ? { composerIntegrationReferences }
            : {}),
          ...(composerArtifactReferences?.length
            ? { composerArtifactReferences }
            : {}),
          ...(base ? { base } : {}),
        });
        const resolvedConversation: ChatConversation = {
          ...result.conversation,
          agentMode: result.conversation.agentMode ?? mode,
        };
        const resolvedConversationId = resolvedConversation.id;
        const optimisticWorkspace = result.workspace;
        const resolvedStoreKey = getAgentConversationStoreKey(resolvedConversation);
        const resolvedOptimisticUserMessage =
          resolvedConversationId === seededConversation.id
            ? seededOptimisticUserMessage
            : buildOptimisticUserMessage({
                conversation: resolvedConversation,
                content,
                runtime: normalizedRuntime,
                ...(optimisticReferenceMetadata
                  ? { metadata: optimisticReferenceMetadata }
                  : {}),
                ...(optimisticAttachments ? { attachments: optimisticAttachments } : {}),
              });
        seedConversationState(
          resolvedConversation,
          optimisticWorkspace ?? null,
          [resolvedOptimisticUserMessage]
        );
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
            uploadedAttachmentIds.length > 0 ? uploadedAttachmentIds : undefined
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
        if (hasGranolaIntegrationReference(composerIntegrationReferences)) {
          if (startIntegrationTab === "granola") {
            onGranolaLinked?.(resolvedConversationId);
          }
          await invalidateAgentConversationGranolaNote(
            queryClient,
            resolvedConversationId,
          );
        }
        await invalidateProjectConversations(targetProjectId);
        handleAutoManagedTitle({
          content,
          conversationId: resolvedConversationId,
          targetProjectId,
          shouldSpawnSessionNamer: true,
          providerHarness: normalizedRuntime.provider,
        });
      } catch (err) {
        if (seededStoreKey) {
          setAgentActivityLabel(seededStoreKey, null);
          setAgentRunning(seededStoreKey, false);
          setSending(seededStoreKey, false);
        }
        removeOptimisticConversation(initialConversation.id, optimisticStoreKey);
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
      onGranolaLinked,
      setSending,
    ]
  );

  return handleStartAgentConversation;
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
    projectId: conversation.contextType === "project" ? conversation.contextId : null,
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

function buildOptimisticMessageAttachments(files: File[]): MessageAttachment[] | undefined {
  if (files.length === 0) {
    return undefined;
  }

  return files.map((file, index) => ({
    id: `optimistic:${index}:${file.name}`,
    fileName: file.name || "attachment",
    fileSize: file.size,
    ...(file.type ? { mimeType: file.type } : {}),
    ...(file.type.startsWith("image/") && typeof URL.createObjectURL === "function"
      ? { previewUrl: URL.createObjectURL(file) }
      : {}),
  }));
}
