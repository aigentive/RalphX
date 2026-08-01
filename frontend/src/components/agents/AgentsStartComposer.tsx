import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Lock, PauseCircle, Sparkles } from "lucide-react";

import type {
  AgentConversationBranchMode,
  AgentConversationBaseSelection,
  AgentConversationWorkspaceMode,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  CapabilityIntent,
  TeamIntent,
} from "@/api/chat";
import { mcpPolicyApi } from "@/api/mcp-policy";
import type { AutomationAuthoringMode } from "@/api/automations";
import type { ComposerRoleDefault } from "@/api/manual-role-defaults.types";
import type { Project } from "@/types/project";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useStartComposerRoleDefault } from "@/hooks/useManualRoleDefaults";
import { useConfirmation } from "@/hooks/useConfirmation";
import { usePersonas } from "@/hooks/usePersonas";
import {
  PERSONA_UNAVAILABLE_PREFIX,
  isPersonaUnavailableError,
} from "@/lib/personaErrors";
import { withAlpha } from "@/lib/theme-colors";
import {
  useAgentSessionStore,
  type AgentEffort,
  type AgentProvider,
  type AgentRuntimeProviderContext,
  type AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import {
  selectComposerDraft,
  useChatStore,
  type ChatComposerAttachment,
  type ChatComposerFolder,
} from "@/stores/chatStore";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  type BranchBaseOption,
} from "@/components/shared/branchBaseOptions";
import {
  agentModelSupportsCodexUltra,
  type AgentModelRegistry,
} from "@/lib/agent-models";
import {
  CODEX_FAST_MODE_DESCRIPTION,
  codexFastModeAvailabilityForProvider,
} from "@/lib/codex-fast-mode";
import {
  AgentComposerProjectLine,
  AgentComposerSurface,
  type AgentComposerSurfaceProps,
} from "./AgentComposerSurface";
import { buildCapabilityOptions } from "./composer/runtime/capabilityOptions";
import {
  buildAgentStartConversationRetryInput,
  parseLinkedSetupFailure,
  parseMcpSetupPreflightFailure,
  type McpSetupPreflightFailureDetails,
} from "./agentStartErrors";
import { AgentProviderSettingsButton } from "./AgentProviderSettingsButton";
import type { AgentQueueHaltState } from "./agentExecutionPause";
import {
  AGENT_PROVIDER_OPTIONS,
  DEFAULT_AGENT_RUNTIME,
  agentEffortOptions,
  agentModelOptions,
  defaultEffortForModel,
  defaultModelForProvider,
  normalizeRuntimeForPersistence,
  normalizeRuntimeSelection,
} from "./agentOptions";
import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
  normalizeRuntimeForSelectableProvider,
  supportedEffortsForProvider,
  supportedModelAliasesForProvider,
} from "./agentProviderAvailability";
import {
  PRIMARY_AGENT_START_MODE_IDS,
  buildAgentStartModeOptions,
} from "./agentStartModeOptions";
import { useUiStore } from "@/stores/uiStore";
import { PersonaUnavailableNotice } from "@/components/personas/PersonaUnavailableNotice";
import { PersonaBuildBanner } from "./PersonaBuildBanner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

interface PendingAttachment {
  id: string;
  file: File;
  fileName: string;
  fileSize: number;
  mimeType?: string;
}

interface AgentsStartComposerSubmitInput {
  projectId: string | null;
  content: string;
  runtime: AgentRuntimeSelection;
  runtimeProviderContext?: AgentRuntimeProviderContext;
  useRoleDefault?: boolean;
  mode: AgentConversationWorkspaceMode;
  automationAuthoringMode?: AutomationAuthoringMode;
  base: AgentConversationBaseSelection | null;
  files: File[];
  folders?: ChatComposerFolder[];
  codexFastMode?: boolean | null;
  personaId?: string | null;
  capabilityIntent?: CapabilityIntent | null;
  teamIntent?: TeamIntent | null;
  sourcePersonaId?: string;
  composerArtifactReferences?: ComposerArtifactReference[] | undefined;
  composerProjectReferences?: ComposerProjectReference[] | undefined;
  composerIntegrationReferences?: ComposerIntegrationReference[] | undefined;
}

interface AgentsStartComposerProps {
  projects: Project[];
  defaultProjectId: string | null;
  defaultRuntime: AgentRuntimeSelection | null;
  executionHaltState?: AgentQueueHaltState;
  isLoadingProjects: boolean;
  isSubmitting: boolean;
  modelRegistry: AgentModelRegistry;
  onRuntimePreferenceChange?: (projectId: string, runtime: AgentRuntimeSelection) => void;
  onSubmit: (input: AgentsStartComposerSubmitInput) => Promise<void>;
}

const MAX_FILES = 5;
const MAX_FILE_SIZE = 10 * 1024 * 1024;
const REVIEW_PR_DEFAULT_PROMPT = "Review this PR.";
const STARTER_TYPING_WORDS = [
  "agent",
  "project",
  "plan",
  "idea",
  "build",
  "PR",
  "feature",
  "bugfix",
] as const;
const STARTER_TYPING_HOLD_MS = 1600;
const STARTER_TYPING_SPEED_MS = 72;
const STARTER_DELETING_SPEED_MS = 44;
const STARTER_TYPING_INITIAL_WORD = STARTER_TYPING_WORDS[0];
const AGENTS_START_COMPOSER_DRAFT_KEY = "agents:start";

type StarterTypingPhase = "holding" | "typing" | "deleting";

type StartComposerError =
  | { kind: "plain"; message: string }
  | { kind: "linked_setup"; message: string }
  | { kind: "mcp_setup"; details: McpSetupPreflightFailureDetails }
  | { kind: "persona_unavailable"; message: string };

function isPendingAttachment(
  attachment: ChatComposerAttachment,
): attachment is PendingAttachment {
  return attachment.file !== undefined;
}

function plainStartComposerError(message: string): StartComposerError {
  return { kind: "plain", message };
}

function copyRuntimeProviderValues(
  values: readonly string[] | null,
): string[] | null {
  return values ? [...values] : null;
}

function startComposerErrorFromUnknown(error: unknown): StartComposerError {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "Failed to start agent conversation";
  if (isPersonaUnavailableError(message)) {
    return {
      kind: "persona_unavailable",
      message: message
        .slice(PERSONA_UNAVAILABLE_PREFIX.length)
        .replace(/\]$/, "")
        .trim(),
    };
  }
  const linked = parseLinkedSetupFailure(error);
  if (linked) {
    return { kind: "linked_setup", message: linked.message };
  }
  const mcpSetup = parseMcpSetupPreflightFailure(error);
  if (mcpSetup) {
    return { kind: "mcp_setup", details: mcpSetup };
  }
  return plainStartComposerError(message);
}

function composerIntegrationReferencesEqual(
  left: ComposerIntegrationReference[],
  right: ComposerIntegrationReference[],
): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((reference, index) => {
    const other = right[index];
    return (
      other !== undefined &&
      reference.provider === other.provider &&
      reference.kind === other.kind &&
      reference.id === other.id &&
      reference.key === other.key
    );
  });
}

export function AgentsStartComposer({
  projects,
  defaultProjectId,
  defaultRuntime,
  executionHaltState = null,
  isLoadingProjects,
  isSubmitting,
  modelRegistry,
  onRuntimePreferenceChange,
  onSubmit,
}: AgentsStartComposerProps) {
  const defaultStartMode = useAgentSessionStore((s) => s.defaultStartMode);
  const initialRuntime = normalizeRuntimeForPersistence(
    defaultRuntime,
    modelRegistry,
  );
  const [projectId, setProjectId] = useState<string | null>(defaultProjectId);
  const [projectLocked, setProjectLocked] = useState(false);
  const [sourcePersonaId, setSourcePersonaId] = useState<string | null>(null);
  const [sourcePersonaName, setSourcePersonaName] = useState<string | null>(null);
  const [provider, setProvider] = useState<AgentProvider>(initialRuntime.provider);
  const [modelId, setModelId] = useState(initialRuntime.modelId);
  const [effort, setEffort] = useState<AgentEffort>(initialRuntime.effort);
  const [mode, setMode] = useState<AgentConversationWorkspaceMode>(
    defaultStartMode,
  );
  const [capabilityMode, setCapabilityMode] = useState<
    CapabilityIntent["coordinationMode"]
  >("solo");
  const [automationAuthoringMode, setAutomationAuthoringMode] =
    useState<AutomationAuthoringMode | null>(null);
  const [startFromOptions, setStartFromOptions] = useState<BranchBaseOption[]>([]);
  const [pullRequestStartFromOptions, setPullRequestStartFromOptions] = useState<
    BranchBaseOption[]
  >([]);
  const [selectedStartFromKey, setSelectedStartFromKey] = useState("");
  const [isStartFromIsolatedBranch, setIsStartFromIsolatedBranch] =
    useState(false);
  const [isLoadingStartFrom, setIsLoadingStartFrom] = useState(false);
  const [isLoadingPullRequestStartFrom, setIsLoadingPullRequestStartFrom] = useState(false);
  const [pullRequestStartFromMessage, setPullRequestStartFromMessage] =
    useState<string | null>(null);
  const [hydratedStartFromProjectId, setHydratedStartFromProjectId] =
    useState<string | null>(null);
  const [isComposerActive, setIsComposerActive] = useState(false);
  const [draftProjectReferences, setDraftProjectReferences] = useState<
    ComposerProjectReference[]
  >([]);
  const [draftIntegrationReferences, setDraftIntegrationReferences] = useState<
    ComposerIntegrationReference[]
  >([]);
  const [composerIntegrationReferences, setComposerIntegrationReferences] = useState<
    ComposerIntegrationReference[]
  >([]);
  const [draftArtifactReferences, setDraftArtifactReferences] = useState<
    ComposerArtifactReference[]
  >([]);
  const clickupTicketToken = useMemo(
    () => clickupTokenFromIntegrationReferences(draftIntegrationReferences),
    [draftIntegrationReferences],
  );
  const [codexFastModeOverride, setCodexFastModeOverride] = useState<
    boolean | null
  >(null);
  const [isResettingRoleDefault, setIsResettingRoleDefault] = useState(false);
  const [personaId, setPersonaId] = useState<string | null>(null);
  const [roleOverrideKey, setRoleOverrideKey] = useState<string | null>(null);
  const [error, setError] = useState<StartComposerError | null>(null);
  const [isRepairingMcp, setIsRepairingMcp] = useState(false);
  const isStartBusy = isRepairingMcp || isSubmitting;
  const startFromRequestRef = useRef(0);
  const pullRequestStartFromRequestRef = useRef(0);
  const userSelectedStartFromRef = useRef(false);
  const lastStartAttemptRef = useRef<AgentsStartComposerSubmitInput | null>(null);
  const openModal = useUiStore((s) => s.openModal);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const { data: featureFlags } = useFeatureFlags();
  const { data: availablePersonas = [] } = usePersonas(
    projectId
      ? { type: "globalAndProject", projectId }
      : { type: "globalOnly" },
  );
  const personaOptions = useMemo(
    () => [
      {
        id: "none",
        label: "No persona",
        description: "Use the role's default instructions without a persona.",
      },
      ...availablePersonas
        .filter(
          (persona) =>
            persona.status === "active" &&
            (persona.projectId === null || persona.projectId === projectId),
        )
        .map((persona) => ({
          id: persona.id,
          label: persona.name,
          description:
            persona.projectId === null ? "Global persona" : "Project persona",
        })),
    ],
    [availablePersonas, projectId],
  );
  const startModeOptions = useMemo(
    () =>
      buildAgentStartModeOptions({
        autopilotEnabled: featureFlags.agentConversationAutopilot ?? false,
      }),
    [featureFlags.agentConversationAutopilot],
  );
  const {
    settings: providerSettings,
    providers: configuredProviders,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders({ refreshRuntime: true });
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const lastBranchBaseSelectionByProjectId = useAgentSessionStore(
    (s) => s.lastBranchBaseSelectionByProjectId
  );
  const setBranchBaseCacheForProject = useAgentSessionStore(
    (s) => s.setBranchBaseCacheForProject
  );
  const setLastBranchBaseSelectionForProject = useAgentSessionStore(
    (s) => s.setLastBranchBaseSelectionForProject
  );
  const startConversationFailure = useAgentSessionStore(
    (s) => s.startConversationFailure
  );
  const setStartConversationFailure = useAgentSessionStore(
    (s) => s.setStartConversationFailure
  );
  const lastModelEffortByProvider = useAgentSessionStore(
    (s) => s.lastModelEffortByProvider
  );
  const persistedRuntimeOverride = useAgentSessionStore((s) =>
    projectId ? s.lastRuntimeByProjectId[projectId] : undefined
  );
  const clearLastRuntimeForProject = useAgentSessionStore(
    (s) => s.clearLastRuntimeForProject
  );
  const startConversationDraft = useAgentSessionStore(
    (s) => s.startConversationDraft
  );
  const consumeStartConversationDraft = useAgentSessionStore(
    (s) => s.consumeStartConversationDraft
  );
  const startComposerDraft = useChatStore(
    selectComposerDraft(AGENTS_START_COMPOSER_DRAFT_KEY)
  );
  const setComposerDraftContent = useChatStore((s) => s.setComposerDraftContent);
  const setComposerDraftAttachments = useChatStore(
    (s) => s.setComposerDraftAttachments
  );
  const setComposerDraftFolders = useChatStore((s) => s.setComposerDraftFolders);
  const clearComposerDraft = useChatStore((s) => s.clearComposerDraft);
  const content = startComposerDraft?.content ?? "";
  const attachments = useMemo(
    () => (startComposerDraft?.attachments ?? []).filter(isPendingAttachment),
    [startComposerDraft?.attachments]
  );
  const folders = useMemo(
    () => startComposerDraft?.folders ?? [],
    [startComposerDraft?.folders]
  );

  const providerSettingsReady =
    !isLoadingProviderSettings && !isPlaceholderProviderSettings;
  const roleDefaultQuery = useStartComposerRoleDefault(projectId, mode);
  const currentRoleKey = `${projectId}:${mode}`;
  const hasLocalRoleOverride = roleOverrideKey === currentRoleKey;
  const hasRoleOverride =
    Boolean(persistedRuntimeOverride) || hasLocalRoleOverride;
  const providerOptions = useMemo(
    () =>
      buildAgentProviderAvailabilityOptions({
        providers: configuredProviders,
        isReady: providerSettingsReady,
        // Remote hosts serve stored config only; availability is decided from `enabled`, and
        // the copy claims configuration rather than CLI validation (which is unavailable).
        mode: isRemoteEnvironment ? "remote" : "local",
      }),
    [configuredProviders, providerSettingsReady, isRemoteEnvironment]
  );
  const normalizedRuntime = useMemo(() => {
    const runtime = normalizeRuntimeForPersistence(
      defaultRuntime ?? DEFAULT_AGENT_RUNTIME,
      modelRegistry
    );
    return normalizeRuntimeSelection(
      runtime,
      modelRegistry,
      supportedEffortsForProvider(providerOptions, runtime.provider),
      supportedModelAliasesForProvider(providerOptions, runtime.provider)
    );
  }, [defaultRuntime, modelRegistry, providerOptions]);
  const selectedProviderSupportedEfforts = useMemo(
    () => supportedEffortsForProvider(providerOptions, provider),
    [provider, providerOptions]
  );
  const selectedProviderSupportedModelAliases = useMemo(
    () => supportedModelAliasesForProvider(providerOptions, provider),
    [provider, providerOptions]
  );
  const selectableRuntime = useMemo(
    () =>
      normalizeRuntimeForSelectableProvider({
        runtime: { provider, modelId, effort },
        providerOptions,
        defaultProvider: toAgentProvider(providerSettings.defaultProvider),
        modelRegistry,
      }),
    [
      effort,
      modelId,
      modelRegistry,
      provider,
      providerOptions,
      providerSettings.defaultProvider,
    ]
  );
  const providerStatusMessage = getProviderAvailabilityMessage({
    provider,
    providerOptions,
    isReady: providerSettingsReady,
  });
  const codexProviderSettings = configuredProviders.find(
    (entry) => entry.provider === "codex",
  );
  const codexProviderFastMode =
    codexProviderSettings?.serviceTier?.trim().toLowerCase() === "fast";
  const codexFastModeAvailability = codexFastModeAvailabilityForProvider({
    provider: codexProviderSettings,
    modelId,
    isReady: providerSettingsReady,
  });
  const codexFastMode = codexFastModeOverride ?? codexProviderFastMode;
  const selectableCodexFastMode =
    provider === "codex" && codexFastModeAvailability.supported
      ? codexFastMode
      : false;
  const codexUltraAvailable = agentModelSupportsCodexUltra(
    provider,
    modelId,
    modelRegistry,
    codexProviderSettings?.ultraSupportedModels,
  );
  const capabilityOptions = useMemo(() => {
    return buildCapabilityOptions({
      teamEnabled: featureFlags.agentConversationTeam,
      workflowsEnabled: featureFlags.agentConversationWorkflows,
      codexUltraAvailable,
    });
  }, [
    codexUltraAvailable,
    featureFlags.agentConversationTeam,
    featureFlags.agentConversationWorkflows,
  ]);

  const runtimeForRoleDefault = useCallback(
    (roleDefault: ComposerRoleDefault) => {
      const nextProvider = toAgentProvider(roleDefault.value.provider);
      if (!nextProvider) {
        throw new Error(
          `Unsupported provider in ${roleDefault.role} default: ${roleDefault.value.provider}`,
        );
      }
      return normalizeRuntimeSelection(
        {
          provider: nextProvider,
          modelId:
            roleDefault.value.model ??
            defaultModelForProvider(
              nextProvider,
              modelRegistry,
              supportedModelAliasesForProvider(providerOptions, nextProvider),
            ),
          ...(roleDefault.value.effort
            ? { effort: roleDefault.value.effort as AgentEffort }
            : {}),
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, nextProvider),
        supportedModelAliasesForProvider(providerOptions, nextProvider),
      );
    },
    [modelRegistry, providerOptions],
  );

  const applyRoleDefault = useCallback(
    (roleDefault: ComposerRoleDefault, preserveRuntime = false) => {
      const nextRuntime = runtimeForRoleDefault(roleDefault);
      if (!preserveRuntime) {
        setProvider(nextRuntime.provider);
        setModelId(nextRuntime.modelId);
        setEffort(nextRuntime.effort);
      }
      setCodexFastModeOverride(
        roleDefault.value.serviceTier === "provider_default"
          ? null
          : roleDefault.value.serviceTier === "fast",
      );
      const nextCapability = roleDefault.value.coordinationMode ?? "solo";
      setCapabilityMode(
        capabilityOptions.some((option) => option.id === nextCapability)
          ? (nextCapability as CapabilityIntent["coordinationMode"])
          : "solo",
      );
      setPersonaId(
        featureFlags.agentPersonas ? roleDefault.value.personaId : null,
      );
    },
    [
      capabilityOptions,
      featureFlags.agentPersonas,
      runtimeForRoleDefault,
    ],
  );

  useEffect(() => {
    if (!roleDefaultQuery.data || hasLocalRoleOverride) {
      return;
    }
    applyRoleDefault(roleDefaultQuery.data, Boolean(persistedRuntimeOverride));
  }, [
    applyRoleDefault,
    hasLocalRoleOverride,
    persistedRuntimeOverride,
    roleDefaultQuery.data,
  ]);

  useEffect(() => {
    const available = capabilityOptions.some(
      (option) => option.id === capabilityMode,
    );
    if (!available) {
      setCapabilityMode("solo");
      if (capabilityMode !== "solo") {
        setError(
          plainStartComposerError(
            "That capability is no longer available, so this draft was switched to Defaults.",
          ),
        );
      }
    }
  }, [capabilityMode, capabilityOptions]);

  const hasSelectableProvider = providerOptions.some((option) => !option.disabled);
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);
  const clearStartError = useCallback(() => {
    lastStartAttemptRef.current = null;
    setStartConversationFailure(null);
    setError(null);
  }, [setStartConversationFailure]);
  const markRoleOverride = useCallback(() => {
    setRoleOverrideKey(currentRoleKey);
  }, [currentRoleKey]);
  const handleCapabilityChange = useCallback(
    async (next: CapabilityIntent["coordinationMode"]) => {
      if (next === capabilityMode) {
        return;
      }
      if (next === "codex_native_ultra") {
        const confirmed = await confirm({
          title: "Enable Codex Ultra?",
          description:
            "Ultra activates provider-native subagents plus maximum reasoning and can dramatically increase total usage. Select it only after considering the cost.",
          confirmText: "Enable Ultra",
        });
        if (!confirmed) {
          return;
        }
      }
      clearStartError();
      markRoleOverride();
      setCapabilityMode(next);
    },
    [capabilityMode, clearStartError, confirm, markRoleOverride],
  );
  const openPersonaSettings = useCallback(() => {
    openModal("settings", { section: "personas" });
  }, [openModal]);

  useEffect(() => {
    if (projectLocked) {
      return;
    }
    setProjectId((currentProjectId) => {
      if (currentProjectId === null && featureFlags.standaloneConversations) {
        return null;
      }
      if (currentProjectId && projects.some((project) => project.id === currentProjectId)) {
        return currentProjectId;
      }
      return defaultProjectId ?? projects[0]?.id ?? null;
    });
  }, [defaultProjectId, featureFlags.standaloneConversations, projectLocked, projects]);

  useEffect(() => {
    if (!featureFlags.standaloneConversations || projectId !== null) {
      return;
    }
    setCapabilityMode("solo");
    setPersonaId(null);
    if (mode !== "chat" && mode !== "persona_builder") {
      setMode("chat");
      setAutomationAuthoringMode(null);
      setError(
        plainStartComposerError(
          "Project-requiring modes are unavailable without a project. Switched to Ask.",
        ),
      );
    }
  }, [featureFlags.standaloneConversations, mode, projectId]);

  useEffect(() => {
    if (mode !== "persona_builder") return;
    setCapabilityMode("solo");
    setPersonaId(null);
  }, [mode]);

  useEffect(() => {
    if (!startConversationDraft) {
      return;
    }
    const draft = consumeStartConversationDraft();
    if (!draft) {
      return;
    }
    setProjectId(draft.projectId);
    setProjectLocked(draft.projectLocked ?? false);
    setSourcePersonaId(draft.sourcePersonaId ?? null);
    setSourcePersonaName(draft.sourcePersonaName ?? null);
    setComposerDraftContent(AGENTS_START_COMPOSER_DRAFT_KEY, draft.content ?? "");
    setMode(draft.mode);
    setAutomationAuthoringMode(draft.automationAuthoringMode ?? null);
    setDraftProjectReferences(draft.composerProjectReferences ?? []);
    setDraftIntegrationReferences(draft.composerIntegrationReferences ?? []);
    setComposerIntegrationReferences(draft.composerIntegrationReferences ?? []);
    setDraftArtifactReferences(draft.composerArtifactReferences ?? []);
    setIsStartFromIsolatedBranch(false);
    userSelectedStartFromRef.current = false;
  }, [
    consumeStartConversationDraft,
    setComposerDraftContent,
    startConversationDraft,
  ]);

  useEffect(() => {
    if (!startConversationFailure) {
      return;
    }
    const { retryInput } = startConversationFailure;
    setProjectId(retryInput.projectId);
    setProvider(retryInput.runtime.provider);
    setModelId(retryInput.runtime.modelId);
    setEffort(retryInput.runtime.effort);
    setMode(retryInput.mode);
    setAutomationAuthoringMode(retryInput.automationAuthoringMode ?? null);
    if (retryInput.base) {
      setIsStartFromIsolatedBranch(retryInput.base.branchMode === "isolated");
    }
    setError(
      startConversationFailure.kind === "linked_setup"
        ? {
            kind: "linked_setup",
            message: startConversationFailure.message,
          }
        : {
            kind: "mcp_setup",
            details: {
              provider: startConversationFailure.provider,
              serverId: startConversationFailure.serverId,
              scope: startConversationFailure.scope,
              conflictKind: startConversationFailure.conflictKind,
              repairStatus: startConversationFailure.repairStatus,
            },
          },
    );
  }, [startConversationFailure]);

  useEffect(() => {
    if (roleDefaultQuery.data && !hasRoleOverride) {
      return;
    }
    setProvider(normalizedRuntime.provider);
    setModelId(normalizedRuntime.modelId);
    setEffort(normalizedRuntime.effort);
  }, [hasRoleOverride, normalizedRuntime, roleDefaultQuery.data]);

  const modelOptions = useMemo(
    () =>
      agentModelOptions(
        provider,
        modelRegistry,
        selectedProviderSupportedModelAliases
      ),
    [modelRegistry, provider, selectedProviderSupportedModelAliases]
  );
  const effortOptions = useMemo(
    () =>
      agentEffortOptions(
        provider,
        modelId,
        modelRegistry,
        selectedProviderSupportedEfforts
      ),
    [modelId, modelRegistry, provider, selectedProviderSupportedEfforts]
  );
  const activeProject = useMemo(
    () => projects.find((project) => project.id === projectId) ?? null,
    [projectId, projects]
  );
  const activeProjectId = activeProject?.id ?? null;
  const activeProjectBaseBranch = activeProject?.baseBranch ?? null;
  const activeProjectWorkingDirectory = activeProject?.workingDirectory ?? null;
  const allStartFromOptions = useMemo(
    () => [...startFromOptions, ...pullRequestStartFromOptions],
    [pullRequestStartFromOptions, startFromOptions]
  );
  const selectedStartFrom =
    allStartFromOptions.find((option) => option.key === selectedStartFromKey) ?? null;
  const reviewPrDefaultPrompt =
    mode === "review_pr" ? REVIEW_PR_DEFAULT_PROMPT : undefined;
  const isExecutionHalted = executionHaltState !== null;
  const executionHaltTitle =
    executionHaltState === "stopped" ? "Execution is stopped" : "Execution is paused";
  const executionHaltDescription =
    executionHaltState === "stopped"
      ? "New prompts will queue until execution starts."
      : "New prompts will queue until execution resumes.";
  const fallbackStartFrom = useMemo<AgentConversationBaseSelection | null>(() => {
    if (!activeProject) {
      return null;
    }
    const ref = activeProject.baseBranch ?? "main";
    return {
      kind: "project_default",
      ref,
      displayName: `Project default (${ref})`,
    };
  }, [activeProject]);
  const selectedStartFromSelection =
    selectedStartFrom?.selection ?? fallbackStartFrom;
  const selectionForcesIsolatedBranch =
    selectedStartFromSelection
      ? startSelectionForcesIsolatedBranch(selectedStartFromSelection)
      : false;
  const startFromForcesIsolatedBranch =
    mode === "review_pr" || selectionForcesIsolatedBranch;
  const effectiveStartFromIsolatedBranch =
    startFromForcesIsolatedBranch || isStartFromIsolatedBranch;

  const persistRuntimePreference = useCallback(
    (nextProjectId: string, runtime: AgentRuntimeSelection) => {
      if (!nextProjectId) {
        return;
      }
      onRuntimePreferenceChange?.(
        nextProjectId,
        normalizeRuntimeSelection(
          runtime,
          modelRegistry,
          supportedEffortsForProvider(providerOptions, runtime.provider),
          supportedModelAliasesForProvider(providerOptions, runtime.provider)
        )
      );
    },
    [modelRegistry, onRuntimePreferenceChange, providerOptions]
  );

  useEffect(() => {
    if (!providerSettingsReady || !selectableRuntime) {
      return;
    }
    if (
      selectableRuntime.provider === provider &&
      selectableRuntime.modelId === modelId &&
      selectableRuntime.effort === effort
    ) {
      return;
    }
    setProvider(selectableRuntime.provider);
    setModelId(selectableRuntime.modelId);
    setEffort(selectableRuntime.effort);
  }, [
    effort,
    modelId,
    provider,
    providerSettingsReady,
    selectableRuntime,
  ]);

  const handleProjectChange = useCallback(
    (nextProjectId: string | null) => {
      if (projectLocked) return;
      clearStartError();
      userSelectedStartFromRef.current = false;
      setIsStartFromIsolatedBranch(false);
      setProjectId(nextProjectId);
      if (nextProjectId) {
        persistRuntimePreference(nextProjectId, { provider, modelId, effort });
      }
    },
    [
      clearStartError,
      effort,
      modelId,
      persistRuntimePreference,
      projectLocked,
      provider,
    ]
  );

  const handleProviderChange = useCallback(
    (nextProvider: AgentProvider) => {
      /* c8 ignore next 3 -- menu options are disabled; keep this guard for programmatic calls. */
      if (providerOptions.find((option) => option.id === nextProvider)?.disabled) {
        return;
      }
      clearStartError();
      markRoleOverride();
      const remembered = lastModelEffortByProvider[nextProvider];
      const nextProviderModelAliases = supportedModelAliasesForProvider(
        providerOptions,
        nextProvider,
      );
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider: nextProvider,
          modelId:
            remembered?.modelId ??
            defaultModelForProvider(
              nextProvider,
              modelRegistry,
              nextProviderModelAliases,
            ),
          effort: remembered?.effort,
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, nextProvider),
        nextProviderModelAliases
      );
      setProvider(nextRuntime.provider);
      setModelId(nextRuntime.modelId);
      setEffort(nextRuntime.effort);
      if (projectId) persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      lastModelEffortByProvider,
      modelRegistry,
      markRoleOverride,
      persistRuntimePreference,
      projectId,
      providerOptions,
    ]
  );

  const handleModelChange = useCallback(
    (nextModelId: string) => {
      clearStartError();
      markRoleOverride();
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider,
          modelId: nextModelId,
          effort: defaultEffortForModel(provider, nextModelId, modelRegistry),
        },
        modelRegistry,
        selectedProviderSupportedEfforts,
        selectedProviderSupportedModelAliases
      );
      setProvider(nextRuntime.provider);
      setModelId(nextRuntime.modelId);
      setEffort(nextRuntime.effort);
      if (projectId) persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      modelRegistry,
      markRoleOverride,
      persistRuntimePreference,
      projectId,
      provider,
      selectedProviderSupportedEfforts,
      selectedProviderSupportedModelAliases,
    ]
  );

  const handleEffortChange = useCallback(
    (nextEffort: AgentEffort) => {
      clearStartError();
      markRoleOverride();
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider,
          modelId,
          effort: nextEffort,
        },
        modelRegistry,
        selectedProviderSupportedEfforts,
        selectedProviderSupportedModelAliases
      );
      setProvider(nextRuntime.provider);
      setModelId(nextRuntime.modelId);
      setEffort(nextRuntime.effort);
      if (projectId) persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      modelId,
      modelRegistry,
      markRoleOverride,
      persistRuntimePreference,
      projectId,
      provider,
      selectedProviderSupportedEfforts,
      selectedProviderSupportedModelAliases,
    ]
  );

  const handleStartFromChange = useCallback(
    (nextKey: string) => {
      clearStartError();
      userSelectedStartFromRef.current = true;
      setSelectedStartFromKey(nextKey);
      const nextSelection =
        allStartFromOptions.find((option) => option.key === nextKey)?.selection ??
        null;
      setIsStartFromIsolatedBranch(
        startSelectionDefaultsToIsolatedBranch(nextSelection)
      );
      if (activeProjectId && !isTransientStartFromKey(nextKey)) {
        setLastBranchBaseSelectionForProject(activeProjectId, nextKey);
      }
    },
    [
      activeProjectId,
      allStartFromOptions,
      clearStartError,
      setLastBranchBaseSelectionForProject,
    ]
  );

  const handleComposerIntegrationReferencesChange = useCallback(
    (references: ComposerIntegrationReference[]) => {
      if (
        !composerIntegrationReferencesEqual(
          composerIntegrationReferences,
          references,
        )
      ) {
        clearStartError();
      }
      setComposerIntegrationReferences(references);
    },
    [clearStartError, composerIntegrationReferences]
  );

  const handleResetRoleDefault = useCallback(async () => {
    clearStartError();
    setIsResettingRoleDefault(true);
    try {
      const result = await roleDefaultQuery.refetch();
      if (result.isError || !result.data) {
        const message =
          result.error instanceof Error
            ? result.error.message
            : "Failed to load the current role default";
        setError(plainStartComposerError(message));
        return;
      }
      if (projectId) {
        clearLastRuntimeForProject(projectId);
      }
      setRoleOverrideKey(null);
      applyRoleDefault(result.data);
    } catch (error) {
      setError(
        plainStartComposerError(
          error instanceof Error
            ? error.message
            : "Failed to load the current role default",
        ),
      );
    } finally {
      setIsResettingRoleDefault(false);
    }
  }, [
    applyRoleDefault,
    clearLastRuntimeForProject,
    clearStartError,
    projectId,
    roleDefaultQuery,
  ]);

  const handleFilesSelected = (files: File[]) => {
    if (attachments.length + files.length > MAX_FILES) {
      setError(plainStartComposerError(`Cannot upload more than ${MAX_FILES} files total`));
      return;
    }

    const oversizedFiles = files.filter((file) => file.size > MAX_FILE_SIZE);
    if (oversizedFiles.length > 0) {
      setError(
        plainStartComposerError(
          `Files exceed 10MB limit: ${oversizedFiles.map((file) => file.name).join(", ")}`
        )
      );
      return;
    }

    clearStartError();
    setComposerDraftAttachments(AGENTS_START_COMPOSER_DRAFT_KEY, [
      ...attachments,
      ...files.map((file) => ({
        id:
          globalThis.crypto?.randomUUID?.() ??
          `${file.name}-${file.size}-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        file,
        fileName: file.name,
        fileSize: file.size,
        ...(file.type ? { mimeType: file.type } : {}),
      })),
    ]);
  };

  useEffect(() => {
    startFromRequestRef.current += 1;
    pullRequestStartFromRequestRef.current += 1;
    setHydratedStartFromProjectId(null);
    setPullRequestStartFromOptions([]);
    setPullRequestStartFromMessage(null);
    setIsLoadingPullRequestStartFrom(false);
    setIsStartFromIsolatedBranch(false);
    userSelectedStartFromRef.current = false;

    if (!activeProjectId || !activeProjectWorkingDirectory) {
      setStartFromOptions([]);
      setSelectedStartFromKey("");
      setIsLoadingStartFrom(false);
      return;
    }

    const {
      branchBaseCacheByProjectId: cachedBranchBaseByProjectId,
      lastBranchBaseSelectionByProjectId: rememberedBranchBaseByProjectId,
    } = useAgentSessionStore.getState();
    const fallback = fallbackBranchBaseOptions(activeProjectBaseBranch);
    const cached = cachedBranchBaseByProjectId[activeProjectId];
    const options = cached?.options.length ? cached.options : fallback.options;
    const preferredKey =
      rememberedBranchBaseByProjectId[activeProjectId] ??
      cached?.selectedKey ??
      fallback.selectedKey;
    const nextSelectedStartFromKey =
      resolveBranchSelectionKey(options, preferredKey) ??
      resolveBranchSelectionKey(options, fallback.selectedKey) ??
      fallback.selectedKey;
    const nextStartFromSelection =
      options.find((option) => option.key === nextSelectedStartFromKey)?.selection ??
      null;
    setStartFromOptions(options);
    setSelectedStartFromKey(nextSelectedStartFromKey);
    setIsStartFromIsolatedBranch(
      startSelectionDefaultsToIsolatedBranch(nextStartFromSelection)
    );
    setIsLoadingStartFrom(false);
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    activeProjectWorkingDirectory,
  ]);

  const searchPullRequestStartFromOptions = useCallback(
    (query: string) => {
      if (!activeProjectId) {
        setPullRequestStartFromOptions([]);
        setPullRequestStartFromMessage(null);
        setIsLoadingPullRequestStartFrom(false);
        return;
      }

      const requestId = ++pullRequestStartFromRequestRef.current;
      setIsLoadingPullRequestStartFrom(true);
      setPullRequestStartFromMessage(null);

      void loadPullRequestBaseOptions({ projectId: activeProjectId, query })
        .then((options) => {
          if (pullRequestStartFromRequestRef.current !== requestId) {
            return;
          }
          setPullRequestStartFromOptions((current) => {
            const selected = current.find(
              (option) => option.key === selectedStartFromKey
            );
            if (
              selected &&
              !options.some((option) => option.key === selected.key)
            ) {
              return [selected, ...options];
            }
            return options;
          });
          setIsLoadingPullRequestStartFrom(false);
        })
        .catch((err) => {
          if (pullRequestStartFromRequestRef.current !== requestId) {
            return;
          }
          setPullRequestStartFromOptions((current) =>
            current.filter((option) => option.key === selectedStartFromKey)
          );
          setPullRequestStartFromMessage(
            err instanceof Error
              ? err.message
              : "Unable to search pull requests"
          );
          setIsLoadingPullRequestStartFrom(false);
        });
    },
    [activeProjectId, selectedStartFromKey]
  );

  const ensureStartFromOptionsLoaded = useCallback(() => {
    if (
      !activeProjectId ||
      !activeProjectWorkingDirectory ||
      isLoadingStartFrom ||
      hydratedStartFromProjectId === activeProjectId
    ) {
      return;
    }

    const requestId = ++startFromRequestRef.current;
    setIsLoadingStartFrom(true);

    void loadBranchBaseOptions({
      projectId: activeProjectId,
      workingDirectory: activeProjectWorkingDirectory,
      projectBaseBranch: activeProjectBaseBranch,
    })
      .then((result) => {
        if (startFromRequestRef.current !== requestId) {
          return;
        }
        const preferredKey =
          (activeProjectId
            ? lastBranchBaseSelectionByProjectId[activeProjectId]
            : null) ??
          selectedStartFromKey ??
          result.selectedKey;
        const normalSelectedKey =
          resolveBranchSelectionKey(result.options, preferredKey) ??
          resolveBranchSelectionKey(result.options, result.selectedKey) ??
          result.selectedKey;
        const clickupCandidates = clickupTicketToken
          ? result.options.filter(
              (option) =>
                option.selection.kind === "local_branch" &&
                containsTicketToken(option.selection.ref, clickupTicketToken),
            )
          : [];
        const clickupSelectedKey =
          !userSelectedStartFromRef.current && clickupCandidates.length === 1
            ? clickupCandidates[0]?.key
            : null;
        const nextBranchSelectedKey = clickupSelectedKey ?? normalSelectedKey;
        setStartFromOptions(result.options);
        setSelectedStartFromKey((currentKey) =>
          currentKey.startsWith("pull_request:")
            ? currentKey
            : nextBranchSelectedKey
        );
        if (clickupSelectedKey) {
          setIsStartFromIsolatedBranch(false);
        }
        setBranchBaseCacheForProject(activeProjectId, result.options, normalSelectedKey);
        if (!clickupSelectedKey) {
          setLastBranchBaseSelectionForProject(activeProjectId, nextBranchSelectedKey);
        }
        setHydratedStartFromProjectId(activeProjectId);
        setIsLoadingStartFrom(false);
      })
      .catch(() => {
        if (startFromRequestRef.current !== requestId) {
          return;
        }
        setHydratedStartFromProjectId(activeProjectId);
        setIsLoadingStartFrom(false);
      });
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    activeProjectWorkingDirectory,
    clickupTicketToken,
    hydratedStartFromProjectId,
    isLoadingStartFrom,
    lastBranchBaseSelectionByProjectId,
    selectedStartFromKey,
    setBranchBaseCacheForProject,
    setLastBranchBaseSelectionForProject,
  ]);

  useEffect(() => {
    if (clickupTicketToken) {
      ensureStartFromOptionsLoaded();
    }
  }, [clickupTicketToken, ensureStartFromOptionsLoaded]);

  const handleRemoveAttachment = (attachmentId: string) => {
    clearStartError();
    setComposerDraftAttachments(
      AGENTS_START_COMPOSER_DRAFT_KEY,
      attachments.filter((attachment) => attachment.id !== attachmentId),
    );
  };

  const handleFoldersSelected = useCallback(
    (nextFolders: ChatComposerFolder[]) => {
      clearStartError();
      setComposerDraftFolders(AGENTS_START_COMPOSER_DRAFT_KEY, nextFolders);
    },
    [clearStartError, setComposerDraftFolders],
  );

  const handleRemoveFolder = useCallback(
    (folderId: string) => {
      handleFoldersSelected(folders.filter((folder) => folder.id !== folderId));
    },
    [folders, handleFoldersSelected],
  );

  const submitStartInput = useCallback(
    async (input: AgentsStartComposerSubmitInput) => {
      const launchRuntime = normalizeRuntimeForSelectableProvider({
        runtime: input.runtime,
        providerOptions,
        defaultProvider: toAgentProvider(providerSettings.defaultProvider),
        modelRegistry,
      });
      if (!providerSettingsReady || !launchRuntime) {
        setError(
          plainStartComposerError(
            providerStatusMessage ??
              "Enable a provider with a validated CLI in Settings."
          )
        );
        return;
      }
      const runtimeProviderContext: AgentRuntimeProviderContext = {
        supportedEfforts: copyRuntimeProviderValues(
          supportedEffortsForProvider(providerOptions, launchRuntime.provider)
        ),
        supportedModelAliases: copyRuntimeProviderValues(
          supportedModelAliasesForProvider(providerOptions, launchRuntime.provider)
        ),
      };
      const launchInput: AgentsStartComposerSubmitInput = {
        ...input,
        runtime: launchRuntime,
        runtimeProviderContext,
      };
      lastStartAttemptRef.current = launchInput;
      setStartConversationFailure(null);
      setError(null);
      try {
        await onSubmit(launchInput);
        lastStartAttemptRef.current = null;
        setStartConversationFailure(null);
        clearComposerDraft(AGENTS_START_COMPOSER_DRAFT_KEY);
      } catch (err) {
        const nextError = startComposerErrorFromUnknown(err);
        setError(nextError);
        if (nextError.kind === "linked_setup") {
          setStartConversationFailure({
            kind: "linked_setup",
            message: nextError.message,
            retryInput: buildAgentStartConversationRetryInput(launchInput),
          });
        } else if (nextError.kind === "mcp_setup") {
          setStartConversationFailure({
            kind: "mcp_setup",
            ...nextError.details,
            retryInput: buildAgentStartConversationRetryInput(launchInput),
          });
        }
      }
    },
    [
      clearComposerDraft,
      modelRegistry,
      onSubmit,
      providerOptions,
      providerSettings.defaultProvider,
      providerSettingsReady,
      providerStatusMessage,
      setStartConversationFailure,
    ]
  );

  const openMcpSettings = useCallback(
    (details: McpSetupPreflightFailureDetails) => {
      openModal("settings", {
        section: "mcp",
        provider: details.provider,
        serverId: details.serverId,
        scope: details.scope,
      });
    },
    [openModal],
  );

  const retryLegacyMcpRepair = useCallback(
    async (details: McpSetupPreflightFailureDetails) => {
      if (
        details.provider !== "claude" ||
        details.serverId !== "ralphx" ||
        details.scope !== "user"
      ) {
        return;
      }
      if (!startConversationFailure || startConversationFailure.kind !== "mcp_setup") {
        return;
      }
      const replayInput: AgentsStartComposerSubmitInput = {
        ...startConversationFailure.retryInput,
        files: attachments.map((attachment) => attachment.file),
        folders: folders.map((folder) => ({ ...folder })),
      };
      setIsRepairingMcp(true);
      try {
        await mcpPolicyApi.retryLegacyRepair({
          provider: "claude",
          serverId: "ralphx",
          scope: "user",
        });
        await submitStartInput(replayInput);
      } catch (repairError) {
        const parsed = startComposerErrorFromUnknown(repairError);
        setError(
          parsed.kind === "mcp_setup"
            ? parsed
            : { kind: "mcp_setup", details },
        );
      } finally {
        setIsRepairingMcp(false);
      }
    },
    [attachments, folders, startConversationFailure, submitStartInput],
  );

  const handleRetryWithIsolatedBranch = useCallback(async () => {
    const lastAttempt =
      lastStartAttemptRef.current ??
      (startConversationFailure
        ? {
            ...startConversationFailure.retryInput,
            files: attachments.map((attachment) => attachment.file),
            folders,
          }
        : null);
    if (!lastAttempt) {
      return;
    }
    const isolatedAttempt: AgentsStartComposerSubmitInput = {
      ...lastAttempt,
      base: lastAttempt.base
        ? {
            ...lastAttempt.base,
            branchMode: "isolated",
          }
        : lastAttempt.base,
    };
    setIsStartFromIsolatedBranch(true);
    await submitStartInput(isolatedAttempt);
  }, [attachments, folders, startConversationFailure, submitStartInput]);

  const handleRemovePersonaAndRetry = useCallback(async () => {
    const lastAttempt = lastStartAttemptRef.current;
    if (!lastAttempt) {
      return;
    }
    setPersonaId(null);
    await submitStartInput({
      ...lastAttempt,
      useRoleDefault: false,
      personaId: null,
    });
  }, [submitStartInput]);

  const handleSubmit: AgentComposerSurfaceProps["onSend"] = async (
    message,
    options,
  ) => {
    if (isStartBusy) {
      return;
    }
    if (projectId === null && !featureFlags.standaloneConversations) {
      setError(plainStartComposerError("Project is required"));
      return;
    }
    if (!message.trim()) {
      setError(plainStartComposerError("Prompt is required"));
      return;
    }
    /* c8 ignore next 3 -- submit is disabled for this state; keep this guard for direct calls. */
    if (!hasSelectableProvider || providerStatusMessage) {
      setError(
        plainStartComposerError(
          providerStatusMessage ?? "Enable a provider with a validated CLI in Settings."
        )
      );
      return;
    }
    if (
      mode === "review_pr" &&
      !selectedStartFromSelection?.sourcePullRequest
    ) {
      setError(plainStartComposerError("Select a pull request to review."));
      return;
    }

    const base = selectedStartFromSelection
      ? {
          ...selectedStartFromSelection,
          branchMode: branchModeForStartSelection(
            selectedStartFromSelection,
            effectiveStartFromIsolatedBranch
          ),
        }
      : null;
    let launchRuntime: AgentRuntimeSelection = { provider, modelId, effort };
    let launchCapabilityMode = capabilityMode;
    let launchCodexFastMode =
      provider === "codex" ? selectableCodexFastMode : null;
    let launchPersonaId =
      featureFlags.agentPersonas && personaId ? personaId : null;
    if (!hasLocalRoleOverride && roleDefaultQuery.data) {
      const resolvedDefault = roleDefaultQuery.data;
      if (!persistedRuntimeOverride) {
        launchRuntime = runtimeForRoleDefault(resolvedDefault);
      }
      const defaultCapability =
        resolvedDefault.value.coordinationMode ?? "solo";
      launchCapabilityMode = capabilityOptions.some(
        (option) => option.id === defaultCapability,
      )
        ? (defaultCapability as CapabilityIntent["coordinationMode"])
        : "solo";
      launchCodexFastMode =
        launchRuntime.provider === "codex"
          ? resolvedDefault.value.serviceTier === "provider_default"
            ? codexProviderFastMode
            : resolvedDefault.value.serviceTier === "fast"
          : null;
      launchPersonaId = featureFlags.agentPersonas
        ? resolvedDefault.value.personaId
        : null;
    }
    const capabilityIntent = {
      coordinationMode: launchCapabilityMode,
    } satisfies CapabilityIntent;
    await submitStartInput({
      projectId,
      content: message.trim(),
      runtime: launchRuntime,
      useRoleDefault: !hasRoleOverride && Boolean(roleDefaultQuery.data),
      mode,
      ...(mode === "automation" && automationAuthoringMode
        ? { automationAuthoringMode }
        : {}),
      base,
      files: attachments.map((attachment) => attachment.file),
      folders,
      codexFastMode: launchCodexFastMode,
      ...(mode !== "persona_builder" && launchPersonaId
        ? { personaId: launchPersonaId }
        : {}),
      ...(mode === "persona_builder" && sourcePersonaId
        ? { sourcePersonaId }
        : {}),
      ...(mode !== "persona_builder" && projectId ? { capabilityIntent } : {}),
      ...(options?.projectReferences?.length
        ? { composerProjectReferences: options.projectReferences }
        : {}),
      ...(options?.integrationReferences?.length
        ? { composerIntegrationReferences: options.integrationReferences }
        : {}),
      ...(options?.artifactReferences?.length
        ? { composerArtifactReferences: options.artifactReferences }
        : {}),
    });
  };

  return (
    <>
    <div className="relative flex h-full w-full items-center justify-center overflow-hidden px-6 py-8 sm:px-8">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div
          className="absolute inset-0"
          style={{
            backgroundImage: `
              linear-gradient(${withAlpha("var(--text-primary)", 4)} 1px, transparent 1px),
              linear-gradient(90deg, ${withAlpha("var(--text-primary)", 4)} 1px, transparent 1px)
            `,
            backgroundSize: "64px 64px",
            opacity: 0.07,
          }}
        />
        <div
          className="absolute left-1/2 top-[17%] h-[180px] w-[min(620px,72vw)] -translate-x-1/2 rounded-full blur-3xl"
          style={{
            background: `radial-gradient(circle, ${withAlpha("var(--accent-primary)", 8)} 0%, transparent 72%)`,
            opacity: 0.28,
          }}
        />
      </div>

      <div className="relative z-10 flex w-full max-w-[980px] flex-col items-center">
        <div className="max-w-[620px] text-center">
          <div
            className="mb-3 inline-flex items-center gap-2 rounded-full border px-3 py-1 text-[0.625rem] font-medium uppercase tracking-[0.16em]"
            style={{
              color: "var(--text-secondary)",
              background: "var(--bg-surface)",
              borderColor: "var(--overlay-weak)",
            }}
          >
            <Sparkles className="h-3.5 w-3.5" style={{ color: "var(--accent-primary)" }} />
            Agent Workspace
          </div>
          <h2
            className="text-[clamp(1.9rem,3.4vw,2.9rem)] font-semibold tracking-[-0.05em] leading-[1.02]"
            style={{ color: "var(--text-primary)" }}
            data-testid="agents-start-heading"
          >
            <span className="inline-flex items-baseline justify-center whitespace-nowrap">
              <span>Start your&nbsp;</span>
              <AnimatedStarterHeadingWord paused={isComposerActive || content.length > 0} />
            </span>
          </h2>
          <p
            className="mx-auto mt-3 max-w-[520px] text-[0.8125rem] leading-relaxed"
            style={{ color: "var(--text-secondary)" }}
          >
            Choose the project and runtime, then ask your agent for something amazing.
          </p>
        </div>

        <div
          className="mt-6 w-full"
          onFocusCapture={() => setIsComposerActive(true)}
          onBlurCapture={(event) => {
            const nextTarget = event.relatedTarget;
            if (!(nextTarget instanceof Node) || !event.currentTarget.contains(nextTarget)) {
              setIsComposerActive(false);
            }
          }}
        >
          {isExecutionHalted && (
            <div
              data-testid="agents-start-paused-banner"
              className="mx-auto mb-3 flex max-w-[620px] items-start gap-3 rounded-md border px-3 py-2.5 text-left"
              style={{
                color: "var(--status-warning)",
                background: "var(--status-warning-muted)",
                borderColor: "var(--status-warning-border)",
              }}
            >
              <PauseCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <div className="min-w-0">
                <p className="text-[13px] font-medium leading-snug">
                  {executionHaltTitle}
                </p>
                <p
                  className="mt-0.5 text-xs leading-relaxed"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {executionHaltDescription}
                </p>
              </div>
            </div>
          )}

          {error?.kind === "persona_unavailable" && (
            <div className="mb-3">
              <PersonaUnavailableNotice
                message={error.message}
                onRemoveAndRetry={() => void handleRemovePersonaAndRetry()}
                onOpenPersonas={openPersonaSettings}
                disabled={isStartBusy}
              />
            </div>
          )}

          {mode === "persona_builder" && (
            <PersonaBuildBanner
              projectName={
                projectId
                  ? projects.find((project) => project.id === projectId)?.name ?? projectId
                  : null
              }
              sourcePersonaName={sourcePersonaName}
            />
          )}

          <AgentComposerSurface
            dataTestId="agents-start-composer"
            textareaTestId="agents-start-textarea"
            actionTestId="agents-start-submit"
            value={content}
            onChange={(value) => {
              clearStartError();
              setComposerDraftContent(AGENTS_START_COMPOSER_DRAFT_KEY, value);
            }}
            onSend={handleSubmit}
            placeholder={
              mode === "review_pr"
                ? REVIEW_PR_DEFAULT_PROMPT
                : mode === "automation"
                  ? "Describe your goal for the new automation"
                  : "Ask the agent to plan, build, debug, or review something"
            }
            isSubmitting={isStartBusy}
            autoFocus
            attachments={attachments}
            folders={folders}
            onFoldersSelected={handleFoldersSelected}
            onRemoveFolder={handleRemoveFolder}
            initialProjectReferences={draftProjectReferences}
            initialIntegrationReferences={draftIntegrationReferences}
            initialArtifactReferences={draftArtifactReferences}
            onIntegrationReferencesChange={handleComposerIntegrationReferencesChange}
            enableAttachments
            onFilesSelected={handleFilesSelected}
            onRemoveAttachment={handleRemoveAttachment}
            attachmentsUploading={isSubmitting && attachments.length > 0}
            submitLabel={
              isExecutionHalted
                ? "Queue Prompt"
                : mode === "automation"
                  ? "Setup Automation"
                  : "Start Agent"
            }
            submittingLabel={
              isExecutionHalted
                ? "Queuing..."
                : mode === "automation"
                  ? "Starting automation..."
                  : "Starting..."
            }
            {...(reviewPrDefaultPrompt
              ? { emptySubmitMessage: reviewPrDefaultPrompt }
              : {})}
            mode={{
              value: mode,
              onValueChange: (value) => {
                clearStartError();
                const nextMode = value as AgentConversationWorkspaceMode;
                if (nextMode !== mode) {
                  if (projectId) {
                    clearLastRuntimeForProject(projectId);
                  }
                  setRoleOverrideKey(null);
                }
                setMode(nextMode);
                if (nextMode !== "automation") {
                  setAutomationAuthoringMode(null);
                }
              },
              options: startModeOptions.filter(
                (option) => option.id !== "persona_builder" || featureFlags.agentPersonas,
              ).map((option) => ({
                ...option,
                ...(projectId === null && option.requiresProject
                  ? {
                      disabled: true,
                      disabledReason: "Requires a project",
                    }
                  : {}),
              })),
              secondaryOptionIds: startModeOptions
                .filter(
                  (option) =>
                    !PRIMARY_AGENT_START_MODE_IDS.includes(option.id),
                )
                .map((option) => option.id),
              testId: "agents-start-mode",
            }}
            {...(mode !== "persona_builder" && projectId && capabilityOptions.length > 1
              ? {
                  capability: {
                    value: capabilityMode,
                    onValueChange: handleCapabilityChange,
                    options: capabilityOptions,
                    testId: "agents-start-capability",
                  },
                }
              : {})}
            {...(mode !== "persona_builder" && featureFlags.agentPersonas && projectId
              ? {
                  persona: {
                    value: personaId ?? "none",
                    onValueChange: (nextPersonaId: string) => {
                      clearStartError();
                      markRoleOverride();
                      setPersonaId(nextPersonaId === "none" ? null : nextPersonaId);
                    },
                    options: personaOptions,
                    footerAction: (
                      <button
                        type="button"
                        className="w-full rounded px-2 py-1.5 text-left text-xs font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)]"
                        onClick={openPersonaSettings}
                      >
                        Manage personas
                      </button>
                    ),
                    testId: "agents-start-persona",
                  },
                }
              : {})}
            project={{
              value: projectId,
              onValueChange: handleProjectChange,
              options: projects.map((project) => ({
                id: project.id,
                label: project.name,
                description: project.workingDirectory,
              })),
              placeholder: projects.length === 0 ? "No projects yet" : "Select project",
              disabled:
                projectLocked ||
                isLoadingProjects ||
                (projects.length === 0 && !featureFlags.standaloneConversations),
              testId: "agents-start-project",
              className: "max-w-[300px] flex-none",
            }}
            provider={{
              value: provider,
              onValueChange: handleProviderChange,
              options: providerOptions.length > 0 ? providerOptions : AGENT_PROVIDER_OPTIONS,
              footerAction: (
                <AgentProviderSettingsButton
                  onClick={openProviderSettings}
                  testId="agents-start-provider-settings"
                />
              ),
              compactFooterAction: (
                <AgentProviderSettingsButton
                  onClick={openProviderSettings}
                  testId="agents-start-provider-settings-compact"
                  compact
                />
              ),
              testId: "agents-start-provider",
              className: "max-w-[172px] flex-none",
            }}
            model={{
              value: modelId,
              onValueChange: handleModelChange,
              options: modelOptions,
              disabled: Boolean(providerStatusMessage),
              onOpenModelSettings: () => openModal("settings", { section: "models" }),
              testId: "agents-start-model",
              className: "max-w-[188px] flex-none",
            }}
            effort={{
              value: effort,
              onValueChange: (value) => handleEffortChange(value as AgentEffort),
              options: effortOptions,
              disabled: Boolean(providerStatusMessage),
              testId: "agents-start-effort",
              className: "max-w-[148px] flex-none",
            }}
            {...(provider === "codex"
              ? {
                  speed: {
                    value:
                      codexFastModeOverride === null
                        ? "provider_default"
                        : codexFastModeOverride
                          ? "fast"
                          : "standard",
                    onValueChange: (value: string) => {
                      markRoleOverride();
                      setCodexFastModeOverride(
                        value === "provider_default" ? null : value === "fast",
                      );
                    },
                    options: [
                      {
                        id: "provider_default",
                        label: "Provider default",
                        description: "Use the service tier configured for Codex.",
                      },
                      {
                        id: "standard",
                        label: "Standard",
                        description: "Use standard processing.",
                      },
                      {
                        id: "fast",
                        label: "Fast",
                        description:
                          codexFastModeAvailability.reason ??
                          CODEX_FAST_MODE_DESCRIPTION,
                        ...(!providerSettingsReady ||
                        !codexFastModeAvailability.supported
                          ? {
                              disabled: true,
                              disabledReason:
                                codexFastModeAvailability.reason ??
                                "Fast processing is unavailable.",
                            }
                          : {}),
                      },
                    ],
                    testId: "agents-start-speed",
                  },
                }
              : {})}
            runtimeDefault={{
              source: roleDefaultQuery.data?.source ?? null,
              isResetting: roleDefaultQuery.isFetching || isResettingRoleDefault,
              disabled: !projectId,
              onReset: handleResetRoleDefault,
            }}
            sendDisabledReason={
              isResettingRoleDefault
                ? "Resetting the current role default"
                : providerStatusMessage
            }
          />

          {providerStatusMessage && (
            <div
              className="mx-auto mt-3 flex max-w-[620px] flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-left text-[0.8125rem]"
              style={{
                color: "var(--text-secondary)",
                background: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
              }}
              data-testid="agents-start-provider-status"
            >
              <span>{providerStatusMessage}</span>
              <button
                type="button"
                className="rounded-md px-2 py-1 text-[0.75rem] font-medium"
                style={{
                  color: "var(--accent-primary)",
                  background: "var(--accent-muted)",
                }}
                onClick={openProviderSettings}
                data-testid="agents-start-provider-status-settings"
              >
                Open Settings
              </button>
            </div>
          )}

          <div className="mt-3 flex w-full flex-wrap items-center justify-between gap-2 px-2">
            <AgentComposerProjectLine
              value={projectId}
              onValueChange={handleProjectChange}
              options={projects.map((project) => ({
                id: project.id,
                label: project.name,
                description: project.workingDirectory,
              }))}
              placeholder={projects.length === 0 ? "No projects yet" : "Select project"}
              disabled={
                projectLocked ||
                isLoadingProjects ||
                (projects.length === 0 && !featureFlags.standaloneConversations)
              }
              testId="agents-start-project"
              allowNoProject={featureFlags.standaloneConversations === true}
              standaloneCaption="Runs in a private workspace"
            />
            {projectLocked && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <span
                    aria-label="Persona build project is locked"
                    className="inline-flex h-7 w-7 items-center justify-center text-[var(--text-muted)]"
                  >
                    <Lock className="h-3.5 w-3.5" aria-hidden="true" />
                  </span>
                </TooltipTrigger>
                <TooltipContent>Persona scope is locked for this build</TooltipContent>
              </Tooltip>
            )}
            {projectId && (
              <BranchBasePicker
                value={selectedStartFromKey}
                onValueChange={handleStartFromChange}
                options={startFromOptions}
                enablePullRequests={Boolean(activeProjectId)}
                pullRequestOptions={pullRequestStartFromOptions}
                isLoadingPullRequests={isLoadingPullRequestStartFrom}
                pullRequestMessage={pullRequestStartFromMessage}
                onPullRequestSearch={searchPullRequestStartFromOptions}
                placeholder={isLoadingStartFrom ? "Loading branch..." : "Base branch"}
                disabled={
                  !activeProjectId || (isLoadingStartFrom && startFromOptions.length === 0)
                }
                testId="agents-start-base"
                isLoading={isLoadingStartFrom}
                onIntent={ensureStartFromOptionsLoaded}
                onOpenChange={(open) => {
                  if (open) {
                    ensureStartFromOptionsLoaded();
                  }
                }}
                closeOnSelect={false}
                isolatedBranch={effectiveStartFromIsolatedBranch}
                isolatedBranchDisabled={startFromForcesIsolatedBranch}
                onIsolatedBranchChange={(value) => {
                  clearStartError();
                  setIsStartFromIsolatedBranch(value);
                }}
              />
            )}
          </div>

          {error?.kind === "mcp_setup" ? (
            <div
              role="alert"
              className="mx-auto mt-4 flex max-w-[620px] flex-col items-start gap-3 rounded-md px-4 py-3 text-left text-[0.8125rem]"
              style={{
                color: "var(--status-warning)",
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--status-warning-border)",
                borderStyle: "solid",
                borderWidth: 1,
              }}
              data-testid="agents-start-mcp-setup-error"
            >
              <div>
                <p className="font-medium leading-snug">
                  {error.details.provider === "claude" ? "Claude" : "Codex"} MCP setup needs attention
                </p>
                <p className="mt-1 leading-relaxed" style={{ color: "var(--text-secondary)" }}>
                  The provider already defines the reserved MCP server ID
                  {" "}<code>{error.details.serverId}</code>
                  {error.details.scope ? ` at ${error.details.scope} scope` : ""}.
                  {error.details.provider === "claude" &&
                  error.details.serverId === "ralphx" &&
                  error.details.scope === "user"
                    ? " RalphX could not remove its reserved Claude user registration. Retry cleanup in the app."
                    : " Open MCP settings to resolve this provider-native conflict."}
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="rounded-md px-3 py-1.5 text-[0.75rem] font-medium"
                  style={{
                    color: "var(--accent-primary)",
                    backgroundColor: "var(--accent-muted)",
                  }}
                  onClick={() => openMcpSettings(error.details)}
                >
                  Open MCP settings
                </button>
                {error.details.repairStatus !== "manual_only" &&
                  error.details.provider === "claude" &&
                  error.details.serverId === "ralphx" &&
                  error.details.scope === "user" && (
                    <button
                      type="button"
                      className="rounded-md px-3 py-1.5 text-[0.75rem] font-medium"
                      style={{
                        color: "var(--text-primary)",
                        backgroundColor: "var(--bg-surface)",
                      }}
                      onClick={() => void retryLegacyMcpRepair(error.details)}
                      disabled={isStartBusy}
                    >
                      {isRepairingMcp ? "Retrying cleanup…" : "Retry cleanup"}
                    </button>
                  )}
              </div>
            </div>
          ) : error?.kind === "linked_setup" ? (
            <div
              className="mx-auto mt-4 flex max-w-[620px] flex-col items-start gap-2 rounded-md border px-4 py-3 text-left text-[0.8125rem]"
              style={{
                color: "var(--status-error)",
                backgroundColor: "var(--status-error-muted)",
                borderColor: "var(--status-error-border)",
                borderStyle: "solid",
                borderWidth: 1,
              }}
              data-testid="agents-start-linked-setup-error"
            >
              <div>
                <p className="font-medium leading-snug">Linked branch setup failed</p>
                <p
                  className="mt-1 leading-relaxed"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {error.message} Branch isolation creates a separate RalphX
                  branch and worktree from the same base, avoiding the checkout
                  conflict.
                </p>
              </div>
              <button
                type="button"
                className="rounded-md px-3 py-1.5 text-[0.75rem] font-medium"
                style={{
                  color: "var(--accent-primary)",
                  backgroundColor: "var(--accent-muted)",
                }}
                onClick={handleRetryWithIsolatedBranch}
                disabled={isStartBusy}
                data-testid="agents-start-linked-setup-retry"
              >
                Retry with isolated branch
              </button>
            </div>
          ) : error ? (
            <div
              className="mx-auto mt-4 inline-flex max-w-full items-center gap-2 rounded-full border px-4 py-2 text-[0.8125rem]"
              style={{
                color: "var(--status-error)",
                backgroundColor: "var(--status-error-muted)",
                borderColor: "var(--status-error-border)",
                borderStyle: "solid",
                borderWidth: 1,
              }}
            >
              {error.message}
            </div>
          ) : null}
        </div>
      </div>
    </div>
    <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

function toAgentProvider(value: unknown): AgentProvider | null {
  return value === "claude" || value === "codex" ? value : null;
}

const AnimatedStarterHeadingWord = memo(function AnimatedStarterHeadingWord({
  paused = false,
}: {
  paused?: boolean;
}) {
  const animatedHeadingWord = useAnimatedStarterWord(paused);

  return (
    <span className="inline-flex items-baseline whitespace-nowrap">
      <span
        data-testid="agents-start-heading-word"
        style={{ color: "var(--accent-primary)" }}
      >
        {animatedHeadingWord}
      </span>
      <span
        aria-hidden="true"
        className="animate-starter-caret ml-0.5 inline-block h-[0.9em] w-[2px] rounded-full align-middle"
        style={{ background: "var(--accent-primary)" }}
      />
    </span>
  );
});

function resolveBranchSelectionKey(
  options: BranchBaseOption[],
  preferredKey: string | null | undefined
) {
  if (!preferredKey) {
    return null;
  }
  return options.some((option) => option.key === preferredKey) ? preferredKey : null;
}

function branchModeForStartSelection(
  selection: AgentConversationBaseSelection,
  isIsolated: boolean
): AgentConversationBranchMode {
  if (selection.kind === "project_default") {
    return "isolated";
  }
  if (selection.kind === "current_branch") {
    return "isolated";
  }
  return isIsolated ? "isolated" : "linked";
}

function startSelectionForcesIsolatedBranch(
  selection: AgentConversationBaseSelection
): boolean {
  return selection.kind === "project_default" || selection.kind === "current_branch";
}

function startSelectionDefaultsToIsolatedBranch(
  selection: AgentConversationBaseSelection | null
): boolean {
  return selection !== null;
}

function isTransientStartFromKey(key: string) {
  return key.startsWith("pull_request:");
}

function clickupTokenFromIntegrationReferences(
  references: ComposerIntegrationReference[],
): string | null {
  const reference = references.find(
    (item) =>
      item.provider.trim().toLowerCase() === "clickup" &&
      item.kind.trim().toLowerCase() === "clickup",
  );
  if (!reference) {
    return null;
  }
  const key = reference.key?.trim();
  if (key) {
    return key;
  }
  const id = reference.id.trim();
  return id ? (id.toUpperCase().startsWith("CU-") ? id : `CU-${id}`) : null;
}

function containsTicketToken(value: string, token: string): boolean {
  const lowerValue = value.toLowerCase();
  const lowerToken = token.toLowerCase();
  let start = lowerValue.indexOf(lowerToken);
  while (start >= 0) {
    const before = start === 0 ? "" : lowerValue[start - 1];
    const after = lowerValue[start + lowerToken.length] ?? "";
    if (!/[a-z0-9]/.test(before ?? "") && !/[a-z0-9]/.test(after)) {
      return true;
    }
    start = lowerValue.indexOf(lowerToken, start + 1);
  }
  return false;
}

function useAnimatedStarterWord(paused = false) {
  const [wordIndex, setWordIndex] = useState(0);
  const [characterCount, setCharacterCount] = useState(
    STARTER_TYPING_INITIAL_WORD.length
  );
  const [phase, setPhase] = useState<StarterTypingPhase>("holding");
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }

    const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const handleChange = () => {
      setPrefersReducedMotion(mediaQuery.matches);
    };

    handleChange();

    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", handleChange);
      return () => mediaQuery.removeEventListener("change", handleChange);
    }

    mediaQuery.addListener(handleChange);
    return () => mediaQuery.removeListener(handleChange);
  }, []);

  useEffect(() => {
    if (paused || prefersReducedMotion) {
      return;
    }

    const currentWord = STARTER_TYPING_WORDS[wordIndex] ?? STARTER_TYPING_INITIAL_WORD;
    const timeoutMs =
      phase === "holding"
        ? STARTER_TYPING_HOLD_MS
        : phase === "typing"
          ? STARTER_TYPING_SPEED_MS
          : STARTER_DELETING_SPEED_MS;

    const timeout = window.setTimeout(() => {
      if (phase === "holding") {
        setPhase("deleting");
        return;
      }

      if (phase === "deleting") {
        if (characterCount > 0) {
          setCharacterCount((current) => current - 1);
          return;
        }

        setWordIndex((current) => (current + 1) % STARTER_TYPING_WORDS.length);
        setPhase("typing");
        return;
      }

      if (characterCount < currentWord.length) {
        setCharacterCount((current) => current + 1);
        return;
      }

      setPhase("holding");
    }, timeoutMs);

    return () => {
      window.clearTimeout(timeout);
    };
  }, [characterCount, paused, phase, prefersReducedMotion, wordIndex]);

  useEffect(() => {
    if (paused || prefersReducedMotion) {
      setWordIndex(0);
      setCharacterCount(STARTER_TYPING_INITIAL_WORD.length);
      setPhase("holding");
      return;
    }

    if (phase === "typing" && characterCount === 0) {
      return;
    }

    const currentWord = STARTER_TYPING_WORDS[wordIndex] ?? STARTER_TYPING_INITIAL_WORD;
    if (phase === "typing" && characterCount > currentWord.length) {
      setCharacterCount(currentWord.length);
    }
  }, [characterCount, paused, phase, prefersReducedMotion, wordIndex]);

  if (paused || prefersReducedMotion) {
    return STARTER_TYPING_INITIAL_WORD;
  }

  return (STARTER_TYPING_WORDS[wordIndex] ?? STARTER_TYPING_INITIAL_WORD).slice(
    0,
    characterCount
  );
}
