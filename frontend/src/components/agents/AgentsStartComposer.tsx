import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { PauseCircle, Sparkles } from "lucide-react";

import type {
  AgentConversationBranchMode,
  AgentConversationBaseSelection,
  AgentConversationWorkspaceMode,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  TeamIntent,
} from "@/api/chat";
import type { Project } from "@/types/project";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
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
} from "@/stores/chatStore";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  type BranchBaseOption,
} from "@/components/shared/branchBaseOptions";
import type { AgentModelRegistry } from "@/lib/agent-models";
import {
  CODEX_FAST_MODE_DESCRIPTION,
  codexFastModeAvailabilityForProvider,
} from "@/lib/codex-fast-mode";
import {
  AgentComposerProjectCreateButton,
  AgentComposerProjectLine,
  AgentComposerSurface,
  type AgentComposerSurfaceProps,
} from "./AgentComposerSurface";
import {
  buildAgentStartConversationRetryInput,
  parseLinkedSetupFailure,
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
import { useUiStore } from "@/stores/uiStore";

interface PendingAttachment {
  id: string;
  file: File;
  fileName: string;
  fileSize: number;
  mimeType?: string;
}

interface AgentsStartComposerSubmitInput {
  projectId: string;
  content: string;
  runtime: AgentRuntimeSelection;
  runtimeProviderContext?: AgentRuntimeProviderContext;
  mode: AgentConversationWorkspaceMode;
  base: AgentConversationBaseSelection | null;
  files: File[];
  codexFastMode?: boolean | null;
  teamIntent?: TeamIntent | null;
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
  onCreateProject: () => void;
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
  | { kind: "linked_setup"; message: string };

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
  const linked = parseLinkedSetupFailure(error);
  if (linked) {
    return { kind: "linked_setup", message: linked.message };
  }
  return plainStartComposerError(
    error instanceof Error ? error.message : "Failed to start agent conversation"
  );
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

const AGENT_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
}> = [
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch." },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request." },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution." },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow." },
  { id: "chat", label: "Chat", description: "Ask read-only questions about the project." },
  { id: "ideation", label: "Ideation", description: "Plan work before creating tasks." },
];

export function AgentsStartComposer({
  projects,
  defaultProjectId,
  defaultRuntime,
  executionHaltState = null,
  isLoadingProjects,
  isSubmitting,
  modelRegistry,
  onCreateProject,
  onRuntimePreferenceChange,
  onSubmit,
}: AgentsStartComposerProps) {
  const initialRuntime = normalizeRuntimeForPersistence(
    defaultRuntime,
    modelRegistry,
  );
  const [projectId, setProjectId] = useState(defaultProjectId ?? "");
  const [provider, setProvider] = useState<AgentProvider>(initialRuntime.provider);
  const [modelId, setModelId] = useState(initialRuntime.modelId);
  const [effort, setEffort] = useState<AgentEffort>(initialRuntime.effort);
  const [mode, setMode] = useState<AgentConversationWorkspaceMode>("edit");
  const [teamEnabled, setTeamEnabled] = useState(false);
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
  const [codexFastModeOverride, setCodexFastModeOverride] = useState<
    boolean | null
  >(null);
  const [error, setError] = useState<StartComposerError | null>(null);
  const startFromRequestRef = useRef(0);
  const pullRequestStartFromRequestRef = useRef(0);
  const userSelectedStartFromRef = useRef(false);
  const lastStartAttemptRef = useRef<AgentsStartComposerSubmitInput | null>(null);
  const openModal = useUiStore((s) => s.openModal);
  const {
    settings: providerSettings,
    providers: configuredProviders,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders({ refreshRuntime: true });
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
  const clearComposerDraft = useChatStore((s) => s.clearComposerDraft);
  const content = startComposerDraft?.content ?? "";
  const attachments = useMemo(
    () => (startComposerDraft?.attachments ?? []).filter(isPendingAttachment),
    [startComposerDraft?.attachments]
  );

  const providerSettingsReady =
    !isLoadingProviderSettings && !isPlaceholderProviderSettings;
  const providerOptions = useMemo(
    () =>
      buildAgentProviderAvailabilityOptions({
        providers: configuredProviders,
        isReady: providerSettingsReady,
      }),
    [configuredProviders, providerSettingsReady]
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
  const hasSelectableProvider = providerOptions.some((option) => !option.disabled);
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);
  const clearStartError = useCallback(() => {
    lastStartAttemptRef.current = null;
    setStartConversationFailure(null);
    setError(null);
  }, [setStartConversationFailure]);

  useEffect(() => {
    setProjectId(defaultProjectId ?? projects[0]?.id ?? "");
  }, [defaultProjectId, projects]);

  useEffect(() => {
    if (!startConversationDraft) {
      return;
    }
    const draft = consumeStartConversationDraft();
    if (!draft) {
      return;
    }
    setProjectId(draft.projectId);
    setComposerDraftContent(AGENTS_START_COMPOSER_DRAFT_KEY, draft.content);
    setMode(draft.mode);
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
    if (retryInput.base) {
      setIsStartFromIsolatedBranch(retryInput.base.branchMode === "isolated");
    }
    setError({
      kind: "linked_setup",
      message: startConversationFailure.message,
    });
  }, [startConversationFailure]);

  useEffect(() => {
    setProvider(normalizedRuntime.provider);
    setModelId(normalizedRuntime.modelId);
    setEffort(normalizedRuntime.effort);
  }, [normalizedRuntime]);

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
    persistRuntimePreference(projectId, selectableRuntime);
  }, [
    effort,
    modelId,
    persistRuntimePreference,
    projectId,
    provider,
    providerSettingsReady,
    selectableRuntime,
  ]);

  const handleProjectChange = useCallback(
    (nextProjectId: string) => {
      clearStartError();
      userSelectedStartFromRef.current = false;
      setIsStartFromIsolatedBranch(false);
      setProjectId(nextProjectId);
      persistRuntimePreference(nextProjectId, { provider, modelId, effort });
    },
    [clearStartError, effort, modelId, persistRuntimePreference, provider]
  );

  const handleProviderChange = useCallback(
    (nextProvider: AgentProvider) => {
      /* c8 ignore next 3 -- menu options are disabled; keep this guard for programmatic calls. */
      if (providerOptions.find((option) => option.id === nextProvider)?.disabled) {
        return;
      }
      clearStartError();
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
      persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      lastModelEffortByProvider,
      modelRegistry,
      persistRuntimePreference,
      projectId,
      providerOptions,
    ]
  );

  const handleModelChange = useCallback(
    (nextModelId: string) => {
      clearStartError();
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
      persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      modelRegistry,
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
      persistRuntimePreference(projectId, nextRuntime);
    },
    [
      clearStartError,
      modelId,
      modelRegistry,
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
        const nextBranchSelectedKey =
          resolveBranchSelectionKey(result.options, preferredKey) ??
          resolveBranchSelectionKey(result.options, result.selectedKey) ??
          result.selectedKey;
        setStartFromOptions(result.options);
        setSelectedStartFromKey((currentKey) =>
          currentKey.startsWith("pull_request:")
            ? currentKey
            : nextBranchSelectedKey
        );
        setBranchBaseCacheForProject(activeProjectId, result.options, nextBranchSelectedKey);
        setLastBranchBaseSelectionForProject(activeProjectId, nextBranchSelectedKey);
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
    hydratedStartFromProjectId,
    isLoadingStartFrom,
    lastBranchBaseSelectionByProjectId,
    selectedStartFromKey,
    setBranchBaseCacheForProject,
    setLastBranchBaseSelectionForProject,
  ]);

  const handleRemoveAttachment = (attachmentId: string) => {
    clearStartError();
    setComposerDraftAttachments(
      AGENTS_START_COMPOSER_DRAFT_KEY,
      attachments.filter((attachment) => attachment.id !== attachmentId),
    );
  };

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

  const handleRetryWithIsolatedBranch = useCallback(async () => {
    const lastAttempt =
      lastStartAttemptRef.current ??
      (startConversationFailure
        ? {
            ...startConversationFailure.retryInput,
            files: attachments.map((attachment) => attachment.file),
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
  }, [attachments, startConversationFailure, submitStartInput]);

  const handleSubmit: AgentComposerSurfaceProps["onSend"] = async (
    message,
    options,
  ) => {
    if (!projectId) {
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
    const teamIntent = teamEnabled
      ? ({ coordinationMode: "rx_native_team" } satisfies TeamIntent)
      : null;
    await submitStartInput({
      projectId,
      content: message.trim(),
      runtime: { provider, modelId, effort },
      mode,
      base,
      files: attachments.map((attachment) => attachment.file),
      codexFastMode: provider === "codex" ? selectableCodexFastMode : null,
      ...(teamIntent ? { teamIntent } : {}),
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
                : "Ask the agent to plan, build, debug, or review something"
            }
            isSubmitting={isSubmitting}
            autoFocus
            attachments={attachments}
            initialProjectReferences={draftProjectReferences}
            initialIntegrationReferences={draftIntegrationReferences}
            initialArtifactReferences={draftArtifactReferences}
            onIntegrationReferencesChange={handleComposerIntegrationReferencesChange}
            enableAttachments
            onFilesSelected={handleFilesSelected}
            onRemoveAttachment={handleRemoveAttachment}
            attachmentsUploading={isSubmitting && attachments.length > 0}
            submitLabel={isExecutionHalted ? "Queue Prompt" : "Start Agent"}
            submittingLabel={isExecutionHalted ? "Queuing..." : "Starting..."}
            {...(reviewPrDefaultPrompt
              ? { emptySubmitMessage: reviewPrDefaultPrompt }
              : {})}
            mode={{
              value: mode,
              onValueChange: (value) => {
                clearStartError();
                setMode(value as AgentConversationWorkspaceMode);
              },
              options: AGENT_MODE_OPTIONS,
              testId: "agents-start-mode",
            }}
            team={{
              enabled: teamEnabled,
              onEnabledChange: (enabled) => {
                clearStartError();
                setTeamEnabled(enabled);
              },
              testId: "agents-start-team",
            }}
            project={{
              value: projectId,
              onValueChange: handleProjectChange,
              options: projects.map((project) => ({
                id: project.id,
                label: project.name,
                description: project.workingDirectory,
              })),
              placeholder: projects.length === 0 ? "No projects yet" : "Select project",
              disabled: isLoadingProjects || projects.length === 0,
              testId: "agents-start-project",
              className: "max-w-[300px] flex-none",
              endAction: (
                <AgentComposerProjectCreateButton
                  onClick={onCreateProject}
                  testId="agents-start-new-project"
                />
              ),
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
              fastMode: {
                visible: provider === "codex",
                value: selectableCodexFastMode,
                onValueChange: setCodexFastModeOverride,
                disabled:
                  !providerSettingsReady ||
                  !codexFastModeAvailability.supported,
                description:
                  codexFastModeAvailability.reason ??
                  CODEX_FAST_MODE_DESCRIPTION,
                testId: "agents-start-codex-fast-mode",
              },
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
            sendDisabledReason={providerStatusMessage}
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
              disabled={isLoadingProjects || projects.length === 0}
              testId="agents-start-project"
            />
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
              disabled={!activeProjectId || (isLoadingStartFrom && startFromOptions.length === 0)}
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
          </div>

          {error?.kind === "linked_setup" ? (
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
                disabled={isSubmitting}
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
