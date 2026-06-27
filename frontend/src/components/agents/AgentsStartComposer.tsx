import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { PauseCircle, Sparkles } from "lucide-react";

import type {
  AgentConversationBaseSelection,
  AgentConversationWorkspaceMode,
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
} from "@/api/chat";
import { ticketingApi, type TicketRef } from "@/api/ticketing";
import type { Project } from "@/types/project";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { withAlpha } from "@/lib/theme-colors";
import {
  useAgentSessionStore,
  type AgentEffort,
  type AgentProvider,
  type AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  ticketAssociationBranchBaseOption,
  ticketCanonicalBranchBaseOption,
  ticketProviderForComposerReference,
  type BranchBaseOption,
} from "@/components/shared/branchBaseOptions";
import type { AgentModelRegistry } from "@/lib/agent-models";
import {
  AgentComposerProjectCreateButton,
  AgentComposerProjectLine,
  AgentComposerSurface,
  type AgentComposerSurfaceProps,
} from "./AgentComposerSurface";
import { AgentProviderSettingsButton } from "./AgentProviderSettingsButton";
import type { AgentQueueHaltState } from "./agentExecutionPause";
import {
  AGENT_PROVIDER_OPTIONS,
  DEFAULT_AGENT_RUNTIME,
  agentEffortOptions,
  agentModelOptions,
  defaultEffortForModel,
  defaultModelForProvider,
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
  onSubmit: (input: {
    projectId: string;
    content: string;
    runtime: AgentRuntimeSelection;
    mode: AgentConversationWorkspaceMode;
    base: AgentConversationBaseSelection | null;
    files: File[];
    codexFastMode?: boolean | null;
    composerArtifactReferences?: ComposerArtifactReference[] | undefined;
    composerProjectReferences?: ComposerProjectReference[] | undefined;
    composerIntegrationReferences?: ComposerIntegrationReference[] | undefined;
  }) => Promise<void>;
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

type StarterTypingPhase = "holding" | "typing" | "deleting";

const AGENT_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
}> = [
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch." },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request." },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution." },
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
  const [projectId, setProjectId] = useState(defaultProjectId ?? "");
  const [provider, setProvider] = useState<AgentProvider>(
    normalizeRuntimeSelection(defaultRuntime).provider
  );
  const [modelId, setModelId] = useState(normalizeRuntimeSelection(defaultRuntime).modelId);
  const [effort, setEffort] = useState<AgentEffort>(
    normalizeRuntimeSelection(defaultRuntime).effort
  );
  const [mode, setMode] = useState<AgentConversationWorkspaceMode>("edit");
  const [startFromOptions, setStartFromOptions] = useState<BranchBaseOption[]>([]);
  const [pullRequestStartFromOptions, setPullRequestStartFromOptions] = useState<
    BranchBaseOption[]
  >([]);
  const [ticketStartFromOption, setTicketStartFromOption] =
    useState<BranchBaseOption | null>(null);
  const [selectedStartFromKey, setSelectedStartFromKey] = useState("");
  const [isLoadingStartFrom, setIsLoadingStartFrom] = useState(false);
  const [isLoadingPullRequestStartFrom, setIsLoadingPullRequestStartFrom] = useState(false);
  const [pullRequestStartFromMessage, setPullRequestStartFromMessage] =
    useState<string | null>(null);
  const [hydratedStartFromProjectId, setHydratedStartFromProjectId] =
    useState<string | null>(null);
  const [content, setContent] = useState("");
  const [isComposerActive, setIsComposerActive] = useState(false);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
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
  const [error, setError] = useState<string | null>(null);
  const startFromRequestRef = useRef(0);
  const pullRequestStartFromRequestRef = useRef(0);
  const ticketStartFromRequestRef = useRef(0);
  const userSelectedStartFromRef = useRef(false);
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
  const lastModelEffortByProvider = useAgentSessionStore(
    (s) => s.lastModelEffortByProvider
  );
  const startConversationDraft = useAgentSessionStore(
    (s) => s.startConversationDraft
  );
  const consumeStartConversationDraft = useAgentSessionStore(
    (s) => s.consumeStartConversationDraft
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
    const runtime = normalizeRuntimeSelection(
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
  const codexProviderFastMode =
    configuredProviders
      .find((entry) => entry.provider === "codex")
      ?.serviceTier?.trim()
      .toLowerCase() === "fast";
  const codexFastMode = codexFastModeOverride ?? codexProviderFastMode;
  const hasSelectableProvider = providerOptions.some((option) => !option.disabled);
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);

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
    setContent(draft.content);
    setMode(draft.mode);
    setDraftProjectReferences(draft.composerProjectReferences ?? []);
    setDraftIntegrationReferences(draft.composerIntegrationReferences ?? []);
    setComposerIntegrationReferences(draft.composerIntegrationReferences ?? []);
    setDraftArtifactReferences(draft.composerArtifactReferences ?? []);
    userSelectedStartFromRef.current = false;
  }, [consumeStartConversationDraft, startConversationDraft]);

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
  const pullRequestOptionsWithTicketStartFrom = useMemo(() => {
    if (!ticketStartFromOption) {
      return pullRequestStartFromOptions;
    }
    return [
      ticketStartFromOption,
      ...pullRequestStartFromOptions.filter(
        (option) => option.key !== ticketStartFromOption.key
      ),
    ];
  }, [pullRequestStartFromOptions, ticketStartFromOption]);
  const allStartFromOptions = useMemo(
    () => [...startFromOptions, ...pullRequestOptionsWithTicketStartFrom],
    [pullRequestOptionsWithTicketStartFrom, startFromOptions]
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
      userSelectedStartFromRef.current = false;
      setProjectId(nextProjectId);
      persistRuntimePreference(nextProjectId, { provider, modelId, effort });
    },
    [effort, modelId, persistRuntimePreference, provider]
  );

  const handleProviderChange = useCallback(
    (nextProvider: AgentProvider) => {
      /* c8 ignore next 3 -- menu options are disabled; keep this guard for programmatic calls. */
      if (providerOptions.find((option) => option.id === nextProvider)?.disabled) {
        return;
      }
      const remembered = lastModelEffortByProvider[nextProvider];
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider: nextProvider,
          modelId: remembered?.modelId ?? defaultModelForProvider(nextProvider, modelRegistry),
          effort: remembered?.effort,
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, nextProvider),
        supportedModelAliasesForProvider(providerOptions, nextProvider)
      );
      setProvider(nextRuntime.provider);
      setModelId(nextRuntime.modelId);
      setEffort(nextRuntime.effort);
      persistRuntimePreference(projectId, nextRuntime);
    },
    [lastModelEffortByProvider, modelRegistry, persistRuntimePreference, projectId, providerOptions]
  );

  const handleModelChange = useCallback(
    (nextModelId: string) => {
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
      userSelectedStartFromRef.current = true;
      setSelectedStartFromKey(nextKey);
      if (activeProjectId && !isTransientStartFromKey(nextKey)) {
        setLastBranchBaseSelectionForProject(activeProjectId, nextKey);
      }
    },
    [activeProjectId, setLastBranchBaseSelectionForProject]
  );

  const handleFilesSelected = (files: File[]) => {
    if (attachments.length + files.length > MAX_FILES) {
      setError(`Cannot upload more than ${MAX_FILES} files total`);
      return;
    }

    const oversizedFiles = files.filter((file) => file.size > MAX_FILE_SIZE);
    if (oversizedFiles.length > 0) {
      setError(
        `Files exceed 10MB limit: ${oversizedFiles.map((file) => file.name).join(", ")}`
      );
      return;
    }

    setError(null);
    setAttachments((current) => [
      ...current,
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
    ticketStartFromRequestRef.current += 1;
    setHydratedStartFromProjectId(null);
    setPullRequestStartFromOptions([]);
    setTicketStartFromOption(null);
    setPullRequestStartFromMessage(null);
    setIsLoadingPullRequestStartFrom(false);
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
    setStartFromOptions(options);
    setSelectedStartFromKey(
      resolveBranchSelectionKey(options, preferredKey) ??
        resolveBranchSelectionKey(options, fallback.selectedKey) ??
        fallback.selectedKey
    );
    setIsLoadingStartFrom(false);
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    activeProjectWorkingDirectory,
  ]);

  useEffect(() => {
    const requestId = ++ticketStartFromRequestRef.current;
    const ticketReference = firstTicketComposerReference(composerIntegrationReferences);
    if (!activeProjectId || !ticketReference) {
      setTicketStartFromOption(null);
      setSelectedStartFromKey((currentKey) =>
        isTicketStartFromKey(currentKey)
          ? `project_default:${activeProjectBaseBranch ?? "main"}`
          : currentKey
      );
      return;
    }

    const fallbackOption = ticketCanonicalBranchBaseOption(ticketReference.reference);
    applyTicketStartFromOption(
      fallbackOption,
      activeProjectBaseBranch,
      userSelectedStartFromRef,
      setTicketStartFromOption,
      setSelectedStartFromKey
    );

    void ticketingApi
      .getTicketAssociations({
        provider: ticketReference.provider,
        ticketRef: ticketReference.ticketRef,
        projectId: activeProjectId,
      })
      .then((associations) => {
        if (ticketStartFromRequestRef.current !== requestId) {
          return;
        }
        const associationOption = preferredTicketAssociationStartFromOption(
          associations.pullRequests
        );
        applyTicketStartFromOption(
          associationOption ?? fallbackOption,
          activeProjectBaseBranch,
          userSelectedStartFromRef,
          setTicketStartFromOption,
          setSelectedStartFromKey
        );
      })
      .catch(() => {
        if (ticketStartFromRequestRef.current !== requestId) {
          return;
        }
        applyTicketStartFromOption(
          fallbackOption,
          activeProjectBaseBranch,
          userSelectedStartFromRef,
          setTicketStartFromOption,
          setSelectedStartFromKey
        );
      });
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    composerIntegrationReferences,
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
    setAttachments((current) => current.filter((attachment) => attachment.id !== attachmentId));
  };

  const handleSubmit: AgentComposerSurfaceProps["onSend"] = async (
    message,
    options,
  ) => {
    if (!projectId) {
      setError("Project is required");
      return;
    }
    if (!message.trim()) {
      setError("Prompt is required");
      return;
    }
    /* c8 ignore next 3 -- submit is disabled for this state; keep this guard for direct calls. */
    if (!hasSelectableProvider || providerStatusMessage) {
      setError(providerStatusMessage ?? "Enable a provider with a validated CLI in Settings.");
      return;
    }

    setError(null);
    try {
      await onSubmit({
        projectId,
        content: message.trim(),
        runtime: { provider, modelId, effort },
        mode,
        base: selectedStartFrom?.selection ?? fallbackStartFrom,
        files: attachments.map((attachment) => attachment.file),
        codexFastMode: provider === "codex" ? codexFastMode : null,
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
      setContent("");
      setAttachments([]);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start agent conversation");
    }
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
            onChange={setContent}
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
            onIntegrationReferencesChange={setComposerIntegrationReferences}
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
              onValueChange: (value) => setMode(value as AgentConversationWorkspaceMode),
              options: AGENT_MODE_OPTIONS,
              testId: "agents-start-mode",
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
                value: codexFastMode,
                onValueChange: setCodexFastModeOverride,
                disabled: !providerSettingsReady,
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
              pullRequestOptions={pullRequestOptionsWithTicketStartFrom}
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
            />
          </div>

          {error && (
            <div
              className="mx-auto mt-4 inline-flex max-w-full items-center gap-2 rounded-full border px-4 py-2 text-[0.8125rem]"
              style={{
                color: "var(--status-error)",
                background: "var(--status-error-muted)",
                borderColor: "var(--status-error-border)",
              }}
            >
              {error}
            </div>
          )}
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

type TicketProvider = "jira" | "linear" | "clickup";

interface TicketComposerStartReference {
  reference: ComposerIntegrationReference;
  provider: TicketProvider;
  ticketRef: TicketRef;
}

function firstTicketComposerReference(
  references: ComposerIntegrationReference[]
): TicketComposerStartReference | null {
  for (const reference of references) {
    const provider = ticketProviderForComposerReference(reference);
    if (!provider) {
      continue;
    }
    const id = reference.id.trim() || reference.key?.trim() || "";
    const key = reference.key?.trim() || undefined;
    if (!id) {
      continue;
    }
    return {
      reference,
      provider,
      ticketRef: {
        provider,
        id,
        ...(key ? { key } : {}),
      },
    };
  }
  return null;
}

function preferredTicketAssociationStartFromOption(
  associations: Array<Parameters<typeof ticketAssociationBranchBaseOption>[0]>
) {
  const ranked = [
    ...associations.filter(
      (association) => association.active && association.prNumber != null
    ),
    ...associations.filter((association) => association.prNumber != null),
    ...associations.filter((association) => association.active),
    ...associations,
  ];
  const seen = new Set<string>();
  for (const association of ranked) {
    const key = `${association.id}:${association.branchName ?? association.subtitle ?? ""}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    const option = ticketAssociationBranchBaseOption(association);
    if (option) {
      return option;
    }
  }
  return null;
}

function applyTicketStartFromOption(
  option: BranchBaseOption | null,
  activeProjectBaseBranch: string | null,
  userSelectedStartFromRef: MutableRefObject<boolean>,
  setTicketStartFromOption: Dispatch<SetStateAction<BranchBaseOption | null>>,
  setSelectedStartFromKey: Dispatch<SetStateAction<string>>
) {
  setTicketStartFromOption(option);
  setSelectedStartFromKey((currentKey) => {
    if (userSelectedStartFromRef.current) {
      return currentKey;
    }
    if (!option) {
      return isTicketStartFromKey(currentKey)
        ? `project_default:${activeProjectBaseBranch ?? "main"}`
        : currentKey;
    }
    if (
      !currentKey ||
      currentKey.startsWith("project_default:") ||
      isTicketStartFromKey(currentKey)
    ) {
      return option.key;
    }
    return currentKey;
  });
}

function isTransientStartFromKey(key: string) {
  return key.startsWith("pull_request:") || isTicketStartFromKey(key);
}

function isTicketStartFromKey(key: string) {
  return key.startsWith("ticket_branch:");
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
