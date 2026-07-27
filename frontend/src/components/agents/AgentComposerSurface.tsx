import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useInputHistory } from "@/hooks/useInputHistory";
import {
  useAgentComposerEntries,
  useAgentComposerIntegrationAvailability,
  useAgentComposerIntegrationResources,
  useAgentComposerPlanReferences,
  useAgentComposerSkills,
  type AgentComposerIntegrationAvailability,
} from "@/hooks/useAgentComposerResources";
import { atlassianApi, type AtlassianResourceSummary } from "@/api/atlassian";
import {
  ArrowUp,
  BookOpen,
  Check,
  ChevronDown,
  FileText,
  FolderOpen,
  CircleOff,
  GitFork,
  Loader2,
  Paperclip,
  Plus,
  Search,
  ScrollText,
  Square,
  Ticket,
} from "lucide-react";

import { useChatAttachmentDrop } from "@/hooks/useChatAttachmentDrop";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import {
  useAddConversationFolderReference,
  useConversationFolderReferences,
  useRemoveConversationFolderReference,
} from "@/hooks/useConversationFolderReferences";
import type { ChatComposerFolder } from "@/stores/chatStore";
import type { CapabilityIntent, TeamIntent } from "@/api/chat";
import type { AgentStatus } from "@/stores/chatStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChatAttachmentDropOverlay } from "@/components/Chat/ChatAttachmentDropOverlay";
import type { MessageFolderReference } from "@/components/Chat/MessageReferences.parse";
import {
  ChatAttachmentGallery,
  type ChatAttachment as ComposerAttachment,
} from "@/components/Chat/ChatAttachmentGallery";
import {
  CHAT_ATTACHMENT_ACCEPTED_TYPES,
  CHAT_ATTACHMENT_MAX_FILE_SIZE,
  CHAT_ATTACHMENT_MAX_FILES,
  validateChatAttachmentFiles,
} from "@/components/Chat/chatAttachmentFiles";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { withAlpha } from "@/lib/theme-colors";
import { extractErrorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";
import {
  appendInternalSkillDirectives,
  detectAgentComposerTrigger,
  extractComposerArtifactTokens,
  extractPastedAtlassianResourceUrls,
  extractComposerSlashSkillTokens,
  normalizeComposerArtifactReferences,
  normalizeComposerIntegrationReferences,
  normalizeComposerProjectReferences,
  removeResolvedAtlassianResourceUrls,
  replaceAgentComposerTrigger,
  type AgentComposerArtifactReference,
  type AgentComposerIntegrationKind,
  type AgentComposerIntegrationReference,
  type AgentComposerProjectReference,
} from "./composer/agentComposerCore";
import {
  AgentComposerCommandMenu,
  type AgentComposerMenuItem,
} from "./composer/AgentComposerCommandMenu";
import { ComposerRuntimeSelector } from "./composer/runtime/ComposerRuntimeSelector";
import { ComposerReferencePill } from "./ComposerReferencePill";
import { FolderReferenceChips } from "./FolderReferenceChips";
import { subscribeToComposerExcerptReferences } from "./artifact-selection/composerExcerptBridge";
import {
  composerExcerptReferenceKey,
  normalizeComposerExcerptReferences,
  type ComposerExcerptReference,
} from "./artifact-selection/artifactSelection.types";
import type {
  ComposerRuntimeCapabilityField,
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimeOption,
  ComposerRuntimePersonaField,
  ComposerRuntimeProviderField,
  ComposerRuntimeSpeedField,
} from "./composer/runtime/runtimeSelectorTypes";
import type { AgentComposerSkill } from "@/api/agent-composer";

type ComposerOption = ComposerRuntimeOption;

const PLAN_REFINE_COMMAND_MESSAGE =
  "Please verify and refine the current plan.";

const COMPOSER_INTEGRATION_ACTIONS: ReadonlyArray<{
  kind: AgentComposerIntegrationKind;
  label: string;
}> = [
  { kind: "jira", label: "Jira" },
  { kind: "confluence", label: "Confluence" },
  { kind: "linear", label: "Linear" },
  { kind: "clickup", label: "ClickUp" },
  { kind: "granola", label: "Granola" },
];

function integrationReferenceKey(
  reference: AgentComposerIntegrationReference,
): string {
  return `${reference.provider}:${reference.kind}:${reference.id}`;
}

function atlassianResourceToIntegrationReference(
  resource: AtlassianResourceSummary,
): AgentComposerIntegrationReference {
  return {
    provider: "atlassian",
    kind: resource.kind,
    id: resource.id,
    ...(resource.key ? { key: resource.key } : {}),
    title: resource.title,
    ...(resource.url ? { url: resource.url } : {}),
  };
}

function getSkillSourceLabel(skill: AgentComposerSkill): string {
  return skill.source === "ralphx-internal"
    ? "RalphX"
    : (skill.providerHarness ?? "native");
}

function getSlashSkillLabel(skill: AgentComposerSkill): string {
  return `/${skill.name}`;
}

function getSkillInsertionText(
  skill: AgentComposerSkill | undefined,
  fallbackName: string,
): string {
  if (!skill) {
    return fallbackName;
  }
  if (skill.source === "ralphx-internal") {
    return skill.invocationValue || skill.name || fallbackName;
  }
  return skill.invocationValue || fallbackName;
}

function skillMatchesComposerQuery(
  skill: AgentComposerSkill,
  query: string,
  label = skill.name,
): boolean {
  if (!query) {
    return true;
  }
  const searchable = [
    label.replace(/^[/$]/, ""),
    skill.name,
    skill.displayName ?? "",
    skill.description ?? "",
    skill.providerHarness ?? "",
    skill.scope ?? "",
  ]
    .join(" ")
    .toLowerCase();
  return searchable.includes(query);
}

interface ProjectFieldConfig {
  value: string | null;
  onValueChange: (value: string | null) => void;
  options: ComposerOption[];
  placeholder: string;
  disabled?: boolean;
  testId?: string;
  className?: string;
  allowNoProject?: boolean;
  standaloneCaption?: string;
}

type ProviderFieldConfig = ComposerRuntimeProviderField;
type ModelFieldConfig = ComposerRuntimeModelField;
type EffortFieldConfig = ComposerRuntimeEffortField;
type PersonaFieldConfig = ComposerRuntimePersonaField;
type SpeedFieldConfig = ComposerRuntimeSpeedField;

interface ModeFieldConfig {
  value: string;
  onValueChange: (value: string) => void;
  onOpen?: () => void | Promise<unknown>;
  options: ComposerOption[];
  secondaryOptionIds?: string[];
  disabled?: boolean;
  testId?: string;
}

export type CapabilityFieldConfig = ComposerRuntimeCapabilityField;

export interface ChatFocusOption {
  id: string;
  label: string;
  description?: string;
  icon?: ComponentType<{ className?: string }>;
  toneColor?: string;
  toneBackground?: string;
  toneBorder?: string;
}

export interface ChatFocusFieldConfig {
  value: string;
  onValueChange: (id: string) => void;
  options: ChatFocusOption[];
  disabled?: boolean;
  testId?: string;
}

export interface AgentComposerQuestionMode {
  optionCount: number;
  multiSelect: boolean;
  onMatchedOptions: (indices: number[]) => void;
}

export interface AgentComposerSendOptions {
  folderReferences?: MessageFolderReference[];
  projectReferences?: AgentComposerProjectReference[];
  integrationReferences?: AgentComposerIntegrationReference[];
  artifactReferences?: AgentComposerArtifactReference[];
  excerptReferences?: ComposerExcerptReference[];
  capabilityIntent?: CapabilityIntent | null;
  teamIntent?: TeamIntent | null;
}

export interface AgentComposerSlashCommand {
  id: string;
  label: `/${string}`;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
  insertText?: string;
  onSelect?: () => Promise<unknown> | unknown;
}

export interface AgentComposerSurfaceProps {
  project: ProjectFieldConfig;
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
  effort: EffortFieldConfig;
  persona?: PersonaFieldConfig;
  speed?: SpeedFieldConfig;
  runtimeDefault?: {
    source?: string | null;
    isResetting?: boolean;
    disabled?: boolean;
    onReset: () => Promise<unknown> | void;
  };
  onSend: (
    message: string,
    options?: AgentComposerSendOptions,
  ) => Promise<void> | void;
  onStop?: (() => Promise<unknown> | void) | undefined;
  placeholder?: string;
  isSubmitting?: boolean;
  agentStatus?: AgentStatus;
  value?: string;
  onChange?: (value: string) => void;
  onFocusChange?: (focused: boolean) => void;
  isReadOnly?: boolean;
  autoFocus?: boolean;
  showHelperText?: boolean;
  /**
   * Collapse the composer into a minimal one-row resting state when it is idle
   * (not focused, empty, no attachments / references / queue).
   * Focusing — or any pending activity — expands it with a soft animation.
   * Defaults to false so the new-conversation / start composer keeps its
   * always-expanded layout.
   */
  collapsible?: boolean;
  questionMode?: AgentComposerQuestionMode;
  hasQueuedMessages?: boolean;
  onEditLastQueued?: (() => void) | undefined;
  attachments?: ComposerAttachment[];
  enableAttachments?: boolean;
  onFilesSelected?: ((files: File[]) => void | Promise<unknown>) | undefined;
  onRemoveAttachment?: ((id: string) => void | Promise<unknown>) | undefined;
  attachmentsUploading?: boolean;
  folders?: ChatComposerFolder[];
  onFoldersSelected?: ((folders: ChatComposerFolder[]) => void) | undefined;
  onRemoveFolder?: ((id: string) => void) | undefined;
  initialProjectReferences?: AgentComposerProjectReference[];
  initialIntegrationReferences?: AgentComposerIntegrationReference[];
  initialArtifactReferences?: AgentComposerArtifactReference[];
  onIntegrationReferencesChange?: (references: AgentComposerIntegrationReference[]) => void;
  mode?: ModeFieldConfig;
  capability?: CapabilityFieldConfig;
  chatFocus?: ChatFocusFieldConfig;
  /** Optional compact control appended to the composer toolbar. */
  personaControl?: ReactNode;
  slashCommands?: AgentComposerSlashCommand[];
  onForkSession?: (() => Promise<unknown> | void) | undefined;
  forkSessionDisabled?: boolean;
  dataTestId?: string;
  textareaTestId?: string;
  actionTestId?: string;
  submitLabel?: string;
  submittingLabel?: string;
  emptySubmitMessage?: string;
  sendDisabledReason?: string | null;
  conversationId?: string | null;
  className?: string;
}

/** Textarea min-height (px) for the default, always-expanded composer. */
const COMPOSER_MIN_HEIGHT = 56;
/** Textarea min-height (px) when a collapsible composer is resting/idle. */
const COMPOSER_COLLAPSED_MIN_HEIGHT = 38;
/** Textarea min-height (px) when a collapsible composer is active/expanded (~3 rows). */
const COMPOSER_EXPANDED_MIN_HEIGHT = 92;
/** Textarea growth ceiling (px) before it scrolls internally. */
const COMPOSER_MAX_HEIGHT = 220;
const EMPTY_PROJECT_REFERENCES: AgentComposerProjectReference[] = [];
const EMPTY_INTEGRATION_REFERENCES: AgentComposerIntegrationReference[] = [];
const EMPTY_ARTIFACT_REFERENCES: AgentComposerArtifactReference[] = [];

export function AgentComposerSurface({
  project,
  provider,
  model,
  effort,
  persona,
  speed,
  runtimeDefault,
  onSend,
  onStop,
  placeholder = "Ask the agent to plan, build, debug, or review something",
  isSubmitting = false,
  agentStatus = "idle",
  value: controlledValue,
  onChange: onChangeProp,
  onFocusChange,
  isReadOnly = false,
  autoFocus = false,
  showHelperText = true,
  collapsible = false,
  questionMode,
  hasQueuedMessages = false,
  onEditLastQueued,
  attachments = [],
  enableAttachments = false,
  onFilesSelected,
  onRemoveAttachment,
  attachmentsUploading = false,
  folders = [],
  onFoldersSelected,
  onRemoveFolder,
  initialProjectReferences = EMPTY_PROJECT_REFERENCES,
  initialIntegrationReferences = EMPTY_INTEGRATION_REFERENCES,
  initialArtifactReferences = EMPTY_ARTIFACT_REFERENCES,
  onIntegrationReferencesChange,
  mode,
  capability,
  chatFocus,
  personaControl,
  slashCommands = [],
  onForkSession,
  forkSessionDisabled = false,
  dataTestId,
  textareaTestId,
  actionTestId,
  submitLabel = "Send",
  submittingLabel = "Sending...",
  emptySubmitMessage,
  sendDisabledReason = null,
  conversationId = null,
  className,
}: AgentComposerSurfaceProps) {
  const { data: featureFlags } = useFeatureFlags();
  const [folderReferencesEnabled, setFolderReferencesEnabled] = useState(false);
  const [folderError, setFolderError] = useState<string | null>(null);
  const addFolderReference = useAddConversationFolderReference();
  const removeFolderReference = useRemoveConversationFolderReference();
  const folderReferences = useConversationFolderReferences(
    conversationId,
    folderReferencesEnabled,
  );
  const isControlled = controlledValue !== undefined;
  const [internalValue, setInternalValue] = useState("");
  const [isFocused, setIsFocused] = useState(false);
  const [cursorPosition, setCursorPosition] = useState(0);
  const [activeMenuIndex, setActiveMenuIndex] = useState(0);
  const [selectedInternalSkillNames, setSelectedInternalSkillNames] = useState<
    Set<string>
  >(() => new Set());
  const textareaAppliedHeightRef = useRef<number | null>(null);
  const [selectedProjectReferences, setSelectedProjectReferences] = useState<
    Map<string, AgentComposerProjectReference>
  >(() => new Map());
  const [selectedIntegrationReferences, setSelectedIntegrationReferences] =
    useState<Map<string, AgentComposerIntegrationReference>>(() => new Map());
  const [selectedArtifactReferences, setSelectedArtifactReferences] = useState<
    Map<string, AgentComposerArtifactReference>
  >(() => new Map());
  const [selectedExcerptReferences, setSelectedExcerptReferences] = useState<
    Map<string, ComposerExcerptReference>
  >(() => new Map());
  const surfaceRef = useRef<HTMLDivElement>(null);
  const runtimePersona =
    persona ??
    (personaControl
      ? {
          value: "conversation",
          onValueChange: () => undefined,
          options: [
            {
              id: "conversation",
              label: "Conversation persona",
              description: "Choose the persona for this conversation below.",
            },
          ],
          footerAction: personaControl,
        }
      : undefined);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const restoreTextareaFocusOnActionMenuCloseRef = useRef(false);
  const restoreTextareaFocusCursorRef = useRef<number | null>(null);
  const hydratedInitialReferencesSignatureRef = useRef<string | null>(null);
  const value = isControlled ? controlledValue : internalValue;
  const textareaValueRef = useRef(value);
  const latestValueRef = useRef(value);
  const [actionMenuOpen, setActionMenuOpen] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const integrationAvailability = useAgentComposerIntegrationAvailability({
    projectId: project.value,
    enabled: actionMenuOpen,
  });
  const isAgentAlive = agentStatus !== "idle";
  const isAgentGenerating = agentStatus === "generating";
  const canSendWhileAgentActive = !isReadOnly && isAgentAlive;
  const shouldShowStop =
    Boolean(onStop) && isAgentGenerating && value.trim().length === 0;
  const emptySubmitValue = emptySubmitMessage?.trim() ?? "";
  const hasSubmittableValue =
    value.trim().length > 0 || emptySubmitValue.length > 0;
  const canSubmit =
    hasSubmittableValue &&
    !isReadOnly &&
    !sendDisabledReason &&
    (!isSubmitting || canSendWhileAgentActive);
  const attachmentDisabled =
    isReadOnly || (isSubmitting && !canSendWhileAgentActive);
  const folderReferencesSupported =
    (mode?.value === "persona_builder" && featureFlags.agentPersonas === true) ||
    (mode?.value !== "persona_builder" && Boolean(project.value?.trim()));
  const effectivePlaceholder = isReadOnly
    ? "Viewing historical state (read-only)"
    : questionMode
      ? `Type 1-${questionMode.optionCount} or a custom response...`
      : placeholder;
  const activeTrigger = useMemo(
    () => detectAgentComposerTrigger(value, cursorPosition),
    [cursorPosition, value],
  );
  const composerAssistEnabled =
    !isReadOnly && !questionMode && Boolean(project.value?.trim());
  const pathQuery = activeTrigger?.kind === "path" ? activeTrigger.query : "";
  const integrationQuery =
    activeTrigger?.kind === "integration" ? activeTrigger.query : "";
  const planQuery = activeTrigger?.kind === "plan" ? activeTrigger.query : "";
  const integrationKind =
    activeTrigger?.kind === "integration"
      ? activeTrigger.integrationKind
      : null;
  const pathEntriesQuery = useAgentComposerEntries({
    projectId: project.value ?? "",
    conversationId,
    query: pathQuery,
    enabled:
      composerAssistEnabled && isFocused && activeTrigger?.kind === "path",
  });
  const integrationResourcesQuery = useAgentComposerIntegrationResources({
    kind: integrationKind ?? null,
    query: integrationQuery,
    enabled:
      composerAssistEnabled &&
      isFocused &&
      activeTrigger?.kind === "integration",
  });
  const planReferencesQuery = useAgentComposerPlanReferences({
    projectId: project.value ?? "",
    query: planQuery,
    enabled:
      composerAssistEnabled && isFocused && activeTrigger?.kind === "plan",
  });
  const skillsQuery = useAgentComposerSkills({
    projectId: project.value ?? "",
    conversationId,
    providerHarness: provider.value,
    mode: mode?.value ?? null,
    enabled: composerAssistEnabled && isFocused,
  });

  useEffect(() => {
    // A collapsible composer must load in its minimal/resting state, so it never
    // auto-focuses on mount (focusing would expand it). The user expands it by
    // clicking. Non-collapsible composers (e.g. the start composer) keep autofocus.
    if (autoFocus && !collapsible && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [autoFocus, collapsible]);

  const matchOptionsFromInput = useCallback(
    (input: string) => {
      if (!questionMode) {
        return;
      }

      const trimmed = input.trim();
      if (!trimmed) {
        questionMode.onMatchedOptions([]);
        return;
      }

      if (questionMode.multiSelect) {
        const parts = trimmed.split(",").map((segment) => segment.trim());
        const allNumeric = parts.every((part) => /^\d+$/.test(part));
        if (!allNumeric) {
          questionMode.onMatchedOptions([]);
          return;
        }
        const indices = parts
          .map((part) => parseInt(part, 10))
          .filter((index) => index >= 1 && index <= questionMode.optionCount)
          .map((index) => index - 1);
        questionMode.onMatchedOptions(indices);
        return;
      }

      if (!/^\d+$/.test(trimmed)) {
        questionMode.onMatchedOptions([]);
        return;
      }

      const optionNumber = parseInt(trimmed, 10);
      if (optionNumber >= 1 && optionNumber <= questionMode.optionCount) {
        questionMode.onMatchedOptions([optionNumber - 1]);
        return;
      }

      questionMode.onMatchedOptions([]);
    },
    [questionMode],
  );

  const setValue = useCallback(
    (nextValue: string) => {
      latestValueRef.current = nextValue;
      if (isControlled) {
        onChangeProp?.(nextValue);
      } else {
        setInternalValue(nextValue);
      }
      matchOptionsFromInput(nextValue);
    },
    [isControlled, matchOptionsFromInput, onChangeProp],
  );

  useEffect(() => {
    latestValueRef.current = value;
  }, [value]);

  const { addEntry: addHistoryEntry, handleHistoryKeyDown } = useInputHistory({
    setValue,
  });

  const clearValue = useCallback((options?: { preserveExcerptReferences?: boolean }) => {
    if (isControlled) {
      onChangeProp?.("");
    } else {
      setInternalValue("");
    }
    setCursorPosition(0);
    setSelectedInternalSkillNames(new Set());
    setSelectedProjectReferences(new Map());
    setSelectedIntegrationReferences(new Map());
    setSelectedArtifactReferences(new Map());
    if (!options?.preserveExcerptReferences) {
      setSelectedExcerptReferences(new Map());
    }
    questionMode?.onMatchedOptions([]);
  }, [isControlled, onChangeProp, questionMode]);

  const markComposerFocused = useCallback(() => {
    setIsFocused(true);
    onFocusChange?.(true);
  }, [onFocusChange]);

  const focusTextareaAtComposerCursor = useCallback(
    (fallbackCursor: number) => {
      const focusTextarea = () => {
        const textarea = textareaRef.current;
        if (!textarea) {
          return;
        }
        const nextCursor =
          restoreTextareaFocusCursorRef.current ?? fallbackCursor;
        textarea.focus();
        textarea.setSelectionRange(nextCursor, nextCursor);
        markComposerFocused();
      };
      window.requestAnimationFrame(() => {
        focusTextarea();
        window.setTimeout(focusTextarea, 0);
      });
    },
    [markComposerFocused],
  );

  useEffect(
    () =>
      subscribeToComposerExcerptReferences(conversationId, (reference) => {
        setSelectedExcerptReferences((current) => {
          const next = normalizeComposerExcerptReferences([
            ...current.values(),
            reference,
          ]);
          return new Map(
            next.map((item) => [composerExcerptReferenceKey(item), item]),
          );
        });
        focusTextareaAtComposerCursor(latestValueRef.current.length);
      }),
    [conversationId, focusTextareaAtComposerCursor],
  );

  const applyComposerText = useCallback(
    (nextValue: string, nextCursor: number) => {
      setValue(nextValue);
      setCursorPosition(nextCursor);
      restoreTextareaFocusCursorRef.current = nextCursor;
      focusTextareaAtComposerCursor(nextCursor);
    },
    [focusTextareaAtComposerCursor, setValue],
  );

  const addSelectedIntegrationReferences = useCallback(
    (references: readonly AgentComposerIntegrationReference[]) => {
      const normalizedReferences =
        normalizeComposerIntegrationReferences(references);
      setSelectedIntegrationReferences((current) => {
        const nextSet = new Map(current);
        for (const reference of normalizedReferences) {
          nextSet.delete(`${reference.kind}:${reference.id}`);
          nextSet.set(integrationReferenceKey(reference), reference);
        }
        return nextSet;
      });
    },
    [],
  );

  const skills = useMemo(
    () => skillsQuery.data?.skills ?? [],
    [skillsQuery.data?.skills],
  );
  const skillByMenuId = useMemo(() => {
    const map = new Map<string, AgentComposerSkill>();
    for (const skill of skills) {
      map.set(`skill:${skill.id}`, skill);
    }
    return map;
  }, [skills]);
  const integrationByMenuId = useMemo(() => {
    const map = new Map<string, AgentComposerIntegrationReference>();
    for (const resource of integrationResourcesQuery.data ?? []) {
      if (integrationKind === "clickup") {
        const key =
          "customId" in resource && resource.customId
            ? resource.customId
            : resource.id;
        const reference: AgentComposerIntegrationReference = {
          provider: "clickup",
          kind: "clickup",
          id: resource.id,
          key,
          ...("name" in resource ? { title: resource.name } : {}),
          ...(resource.url ? { url: resource.url } : {}),
        };
        map.set(`integration:clickup:${resource.id}`, reference);
        continue;
      }
      if (integrationKind === "linear") {
        if (!("key" in resource) || !("title" in resource)) {
          continue;
        }
        const reference: AgentComposerIntegrationReference = {
          provider: "linear",
          kind: "linear",
          id: resource.id,
          ...(resource.key ? { key: resource.key } : {}),
          title: resource.title,
          ...(resource.url ? { url: resource.url } : {}),
        };
        map.set(`integration:linear:${resource.id}`, reference);
        continue;
      }
      if (integrationKind === "granola") {
        if (!("title" in resource)) {
          continue;
        }
        const reference: AgentComposerIntegrationReference = {
          provider: "granola",
          kind: "note",
          id: resource.id,
          ...(resource.title ? { title: resource.title } : {}),
          ...(resource.url ? { url: resource.url } : {}),
          ...("summary" in resource && resource.summary
            ? { summaryExcerpt: resource.summary }
            : {}),
          includeTranscript: true,
        };
        map.set(`integration:granola:${resource.id}`, reference);
        continue;
      }
      if (!("kind" in resource)) {
        continue;
      }
      const reference: AgentComposerIntegrationReference = {
        provider: "atlassian",
        kind: resource.kind,
        id: resource.id,
        ...(resource.key ? { key: resource.key } : {}),
        title: resource.title,
        ...(resource.url ? { url: resource.url } : {}),
      };
      map.set(`integration:${resource.kind}:${resource.id}`, reference);
    }
    return map;
  }, [integrationKind, integrationResourcesQuery.data]);
  const planReferenceByMenuId = useMemo(() => {
    const map = new Map<string, AgentComposerArtifactReference>();
    for (const plan of planReferencesQuery.data?.plans ?? []) {
      map.set(`plan:${plan.artifactId}`, {
        artifactId: plan.artifactId,
        kind: "plan",
        ...(plan.title ? { title: plan.title } : {}),
        sessionId: plan.sessionId,
        version: plan.artifactVersion,
        status: plan.status,
      });
    }
    return map;
  }, [planReferencesQuery.data?.plans]);
  const selectedProjectReferenceList = useMemo(
    () =>
      normalizeComposerProjectReferences([
        ...selectedProjectReferences.values(),
      ]),
    [selectedProjectReferences],
  );
  const selectedIntegrationReferenceList = useMemo(
    () =>
      normalizeComposerIntegrationReferences([
        ...selectedIntegrationReferences.values(),
      ]),
    [selectedIntegrationReferences],
  );
  const selectedArtifactReferenceList = useMemo(
    () =>
      normalizeComposerArtifactReferences([
        ...selectedArtifactReferences.values(),
      ]),
    [selectedArtifactReferences],
  );
  const selectedExcerptReferenceList = useMemo(
    () => normalizeComposerExcerptReferences([...selectedExcerptReferences.values()]),
    [selectedExcerptReferences],
  );
  useEffect(() => {
    onIntegrationReferencesChange?.(selectedIntegrationReferenceList);
  }, [onIntegrationReferencesChange, selectedIntegrationReferenceList]);
  useEffect(() => {
    const projectReferences = normalizeComposerProjectReferences(
      initialProjectReferences,
    );
    const integrationReferences = normalizeComposerIntegrationReferences(
      initialIntegrationReferences,
    );
    const artifactReferences = normalizeComposerArtifactReferences(
      initialArtifactReferences,
    );
    const signature = JSON.stringify({
      projectReferences,
      integrationReferences,
      artifactReferences,
    });
    if (hydratedInitialReferencesSignatureRef.current === signature) {
      return;
    }
    hydratedInitialReferencesSignatureRef.current = signature;
    setSelectedProjectReferences(
      new Map(projectReferences.map((reference) => [reference.path, reference])),
    );
    setSelectedIntegrationReferences(
      new Map(
        integrationReferences.map((reference) => [
          `${reference.provider}:${reference.kind}:${reference.id}`,
          reference,
        ]),
      ),
    );
    setSelectedArtifactReferences(
      new Map(
        artifactReferences.map((reference) => [
          `${reference.kind}:${reference.artifactId}`,
          reference,
        ]),
      ),
    );
  }, [
    initialArtifactReferences,
    initialIntegrationReferences,
    initialProjectReferences,
  ]);
  const slashCommandByMenuId = useMemo(() => {
    const map = new Map<string, AgentComposerSlashCommand>();
    for (const command of slashCommands) {
      map.set(`command:custom:${command.id}`, command);
    }
    return map;
  }, [slashCommands]);
  const hasSelectedReferences =
    selectedProjectReferenceList.length > 0 ||
    selectedIntegrationReferenceList.length > 0 ||
    selectedArtifactReferenceList.length > 0 ||
    selectedExcerptReferenceList.length > 0;

  // Collapsed (minimal) resting state. The composer expands when the textarea
  // is focused (cursor active, even with no text yet) or when there is real
  // composer-local content to keep visible: text, attachments, references,
  // queued messages, a pending question, or read-only review. A running agent
  // alone does not keep the prompt expanded after blur; the Stop action remains
  // available in the compact footer. Transient popovers (+, mode, model)
  // intentionally do NOT expand it on their own, so opening a menu on an
  // unfocused composer never resizes the chat block.
  const hasComposerActivity =
    value.trim().length > 0 ||
    attachments.length > 0 ||
    attachmentsUploading ||
    hasSelectedReferences ||
    hasQueuedMessages ||
    Boolean(questionMode) ||
    isReadOnly;
  const isCollapsed = collapsible && !isFocused && !hasComposerActivity;
  const isExpanded = !isCollapsed;
  const compact = isCollapsed;

  // Auto-resize the textarea to its content, floored at a min-height that
  // depends on the collapsed/expanded state and capped before it scrolls.
  // Re-runs on collapse changes so focus/blur animates the 1-row ↔ 3-row swap.
  const textareaMinHeight = collapsible
    ? isExpanded
      ? COMPOSER_EXPANDED_MIN_HEIGHT
      : COMPOSER_COLLAPSED_MIN_HEIGHT
    : COMPOSER_MIN_HEIGHT;
  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }
    textareaValueRef.current = value;

    textarea.style.height = "auto";
    const nextHeight = Math.min(textarea.scrollHeight, COMPOSER_MAX_HEIGHT);
    const appliedHeight = Math.max(nextHeight, textareaMinHeight);
    textarea.style.height = `${appliedHeight}px`;

    if (textareaAppliedHeightRef.current === null) {
      textareaAppliedHeightRef.current = appliedHeight;
      return;
    }
    if (textareaAppliedHeightRef.current === appliedHeight) {
      return;
    }
    textareaAppliedHeightRef.current = appliedHeight;
  }, [value, textareaMinHeight]);

  const slashCommandItems = useMemo<AgentComposerMenuItem[]>(() => {
    const items: AgentComposerMenuItem[] = [];
    if (mode && !mode.disabled) {
      const modeCommands: Record<string, string> = {
        edit: "agent",
        chat: "chat",
        plan: "plan",
        ideation: "ideation",
      };
      for (const option of mode.options) {
        const commandName = modeCommands[option.id] ?? option.id;
        items.push({
          id: `command:mode:${option.id}`,
          kind: "slash-command",
          label: `/${commandName}`,
          description:
            option.description ?? `Switch composer mode to ${option.label}`,
          detail: `mode:${option.id}`,
          ...(option.disabled !== undefined
            ? { disabled: option.disabled }
            : {}),
        });
      }
    }
    if (mode?.value === "plan" && !mode.disabled) {
      items.push({
        id: "command:plan:refine",
        kind: "slash-command",
        label: "/refine",
        description: "Verify and refine the current plan",
        detail: "plan:refine",
      });
    }
    for (const command of slashCommands) {
      const item: AgentComposerMenuItem = {
        id: `command:custom:${command.id}`,
        kind: "slash-command",
        label: command.label,
        detail: `custom:${command.id}`,
        ...(command.disabled !== undefined
          ? { disabled: command.disabled }
          : {}),
      };
      const description = command.disabledReason ?? command.description;
      if (description) {
        item.description = description;
      }
      items.push(item);
    }
    items.push({
      id: "command:clear",
      kind: "slash-command",
      label: "/clear",
      description: "Clear the current prompt",
      detail: "clear",
    });
    for (const skill of skills) {
      if (!skill.enabled) {
        continue;
      }
      const label = getSlashSkillLabel(skill);
      const item: AgentComposerMenuItem = {
        id: `skill:${skill.id}`,
        kind: "skill",
        label,
        detail: skill.name,
        sourceLabel: getSkillSourceLabel(skill),
      };
      if (skill.description) {
        item.description = skill.description;
      }
      items.push(item);
    }
    return items;
  }, [mode, skills, slashCommands]);
  const menuItems = useMemo<AgentComposerMenuItem[]>(() => {
    if (!activeTrigger) {
      return [];
    }
    const query = activeTrigger.query.trim().toLowerCase();
    if (activeTrigger.kind === "path") {
      return (pathEntriesQuery.data?.entries ?? []).map((entry) => ({
        id: `path:${entry.path}`,
        kind: "path" as const,
        label: `@${entry.path}`,
        description:
          entry.parentPath ?? (entry.kind === "directory" ? "Folder" : "File"),
        detail: entry.kind,
      }));
    }
    if (activeTrigger.kind === "plan") {
      return (planReferencesQuery.data?.plans ?? []).map((plan) => ({
        id: `plan:${plan.artifactId}`,
        kind: "plan" as const,
        label: `@plan:${plan.title ?? plan.artifactId}`,
        description: [
          formatPlanReferenceStatus(plan.status),
          `v${plan.artifactVersion}`,
          shortReferenceId(plan.sessionId),
        ].join(" · "),
        detail: plan.status,
        sourceLabel: "Plan",
      }));
    }
    if (activeTrigger.kind === "skill") {
      return skills
        .filter((skill) => skill.enabled)
        .filter((skill) => skillMatchesComposerQuery(skill, query))
        .slice(0, 12)
        .map((skill) => {
          const item: AgentComposerMenuItem = {
            id: `skill:${skill.id}`,
            kind: "skill",
            label: getSlashSkillLabel(skill),
            detail: skill.name,
            sourceLabel: getSkillSourceLabel(skill),
          };
          if (skill.description) {
            item.description = skill.description;
          }
          return item;
        });
    }
    if (activeTrigger.kind === "integration") {
      if (integrationResourcesQuery.isError) {
        return [];
      }
      return (integrationResourcesQuery.data ?? []).map((resource) => {
        const description =
          integrationKind === "clickup"
            ? "name" in resource
              ? resource.name
              : undefined
            : "title" in resource
              ? resource.title
              : undefined;
        const item: AgentComposerMenuItem = {
          id:
            integrationKind === "clickup"
              ? `integration:clickup:${resource.id}`
              : integrationKind === "linear"
                ? `integration:linear:${resource.id}`
                : integrationKind === "granola"
                  ? `integration:granola:${resource.id}`
                  : "kind" in resource
                    ? `integration:${resource.kind}:${resource.id}`
                    : `integration:unknown:${resource.id}`,
          kind: "integration",
          label:
            integrationKind === "clickup"
              ? `@clickup:${"customId" in resource && resource.customId ? resource.customId : resource.id}`
              : integrationKind === "linear"
                ? `@linear:${"key" in resource ? (resource.key ?? resource.id) : resource.id}`
                : integrationKind === "granola"
                  ? `@granola:${resource.id}`
                  : "kind" in resource && resource.kind === "jira"
                    ? `@jira:${resource.key ?? resource.id}`
                    : `@confluence:${resource.id}`,
          detail:
            integrationKind === "clickup"
              ? "statusName" in resource
                ? (resource.statusName ?? "clickup")
                : "clickup"
              : integrationKind === "linear"
                ? "stateName" in resource
                  ? (resource.stateName ?? "linear")
                  : "linear"
                : integrationKind === "granola"
                  ? "Granola"
                  : "kind" in resource
                    ? resource.kind
                    : "integration",
          sourceLabel:
            integrationKind === "clickup"
              ? "ClickUp"
              : integrationKind === "linear"
                ? "Linear"
                : integrationKind === "granola"
                  ? "Granola"
                  : "kind" in resource && resource.kind === "jira"
                    ? "Jira"
                    : "Confluence",
        };
        if (description) {
          item.description = description;
        }
        return item;
      });
    }
    return slashCommandItems
      .filter((item) => {
        if (!query) {
          return true;
        }
        if (item.kind === "skill") {
          const skill = skillByMenuId.get(item.id);
          return skill
            ? skillMatchesComposerQuery(skill, query, item.label)
            : item.label.replace(/^[/$]/, "").toLowerCase().includes(query);
        }
        return item.label.replace(/^[/$]/, "").toLowerCase().includes(query);
      })
      .slice(0, 12);
  }, [
    activeTrigger,
    integrationKind,
    integrationResourcesQuery.data,
    integrationResourcesQuery.isError,
    pathEntriesQuery.data?.entries,
    planReferencesQuery.data?.plans,
    skills,
    skillByMenuId,
    slashCommandItems,
  ]);
  const integrationSearchErrorLabel = useMemo(() => {
    if (!integrationResourcesQuery.isError) {
      return null;
    }
    const target =
      integrationKind === "clickup"
        ? "ClickUp"
        : integrationKind === "linear"
          ? "Linear"
          : integrationKind === "granola"
            ? "Granola"
          : integrationKind === "confluence"
            ? "Confluence"
            : "Jira";
    const message = extractErrorMessage(
      integrationResourcesQuery.error,
      `Unable to search ${target}`,
    );
    return `${target} search failed: ${message}`;
  }, [
    integrationKind,
    integrationResourcesQuery.error,
    integrationResourcesQuery.isError,
  ]);
  const shouldShowCommandMenu =
    composerAssistEnabled && isFocused && Boolean(activeTrigger);
  const integrationEmptyLabel =
    integrationSearchErrorLabel ??
    (integrationQuery.trim()
      ? "No matching integration items"
      : "Type to search Jira, Linear, ClickUp, Granola, or Confluence");
  const menuEmptyLabel =
    activeTrigger?.kind === "path"
      ? "No matching files or folders"
      : activeTrigger?.kind === "plan"
        ? planQuery.trim()
          ? "No matching plans"
          : "Type to search plans"
        : activeTrigger?.kind === "skill"
          ? "No matching skills"
          : activeTrigger?.kind === "integration"
            ? integrationEmptyLabel
            : "No matching commands";
  const menuLoading =
    activeTrigger?.kind === "path"
      ? pathEntriesQuery.isFetching
      : activeTrigger?.kind === "plan"
        ? planReferencesQuery.isFetching
        : activeTrigger?.kind === "skill"
          ? skillsQuery.isFetching
          : activeTrigger?.kind === "integration"
            ? integrationResourcesQuery.isFetching
            : false;

  useEffect(() => {
    setActiveMenuIndex(0);
  }, [activeTrigger?.kind, activeTrigger?.query, menuItems.length]);

  const selectMenuItem = useCallback(
    (item: AgentComposerMenuItem) => {
      if (!activeTrigger || item.disabled) {
        return;
      }
      if (item.kind === "path") {
        const path = item.label.startsWith("@")
          ? item.label.slice(1)
          : item.label;
        setSelectedProjectReferences((current) => {
          const nextSet = new Map(current);
          nextSet.set(path, {
            path,
            kind: item.detail === "directory" ? "directory" : "file",
          });
          return nextSet;
        });
        const next = replaceAgentComposerTrigger(value, activeTrigger, "");
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.kind === "plan") {
        const reference = planReferenceByMenuId.get(item.id);
        if (!reference) {
          return;
        }
        setSelectedArtifactReferences((current) => {
          const nextSet = new Map(current);
          nextSet.set(`${reference.kind}:${reference.artifactId}`, reference);
          return nextSet;
        });
        const next = replaceAgentComposerTrigger(value, activeTrigger, "");
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.kind === "skill") {
        const skill = skillByMenuId.get(item.id);
        const skillName = skill?.name ?? item.detail;
        if (!skillName) {
          return;
        }
        if (skill?.source === "ralphx-internal") {
          setSelectedInternalSkillNames((current) => {
            const nextSet = new Set(current);
            nextSet.add(skill.invocationValue || skill.name);
            return nextSet;
          });
        }
        const replacement = getSkillInsertionText(skill, skillName);
        const next = replaceAgentComposerTrigger(
          value,
          activeTrigger,
          `${replacement} `,
        );
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.kind === "integration") {
        const reference = integrationByMenuId.get(item.id);
        if (!reference) {
          return;
        }
        addSelectedIntegrationReferences([reference]);
        const next = replaceAgentComposerTrigger(value, activeTrigger, "");
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.detail === "clear") {
        clearValue();
        return;
      }
      if (item.detail === "plan:refine") {
        if ((isSubmitting && !canSendWhileAgentActive) || isReadOnly || sendDisabledReason) {
          return;
        }
        addHistoryEntry(PLAN_REFINE_COMMAND_MESSAGE);
        clearValue();
        void Promise.resolve(onSend(PLAN_REFINE_COMMAND_MESSAGE)).catch(
          () => {},
        );
        return;
      }
      if (item.detail?.startsWith("skill:")) {
        const skill = skillByMenuId.get(item.detail);
        if (skill?.source === "ralphx-internal") {
          setSelectedInternalSkillNames((current) => {
            const nextSet = new Set(current);
            nextSet.add(skill.invocationValue || skill.name);
            return nextSet;
          });
        }
        const replacement = getSkillInsertionText(skill, item.label);
        const next = replaceAgentComposerTrigger(
          value,
          activeTrigger,
          `${replacement} `,
        );
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.detail?.startsWith("custom:")) {
        const command = slashCommandByMenuId.get(item.id);
        if (!command) {
          return;
        }
        if (command.onSelect) {
          const next = replaceAgentComposerTrigger(value, activeTrigger, "");
          applyComposerText(next.text, next.cursor);
          void Promise.resolve(command.onSelect()).catch(() => {
            // Parent command handlers surface their own errors.
          });
          return;
        }
        const replacement = command.insertText ?? `${command.label} `;
        const next = replaceAgentComposerTrigger(
          value,
          activeTrigger,
          replacement,
        );
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.detail?.startsWith("mode:") && mode && !mode.disabled) {
        const nextMode = item.detail.slice("mode:".length);
        const option = mode.options.find(
          (candidate) => candidate.id === nextMode,
        );
        if (option && !option.disabled) {
          mode.onValueChange(nextMode);
        }
        const next = replaceAgentComposerTrigger(value, activeTrigger, "");
        applyComposerText(next.text, next.cursor);
      }
    },
    [
      activeTrigger,
      addHistoryEntry,
      addSelectedIntegrationReferences,
      applyComposerText,
      canSendWhileAgentActive,
      clearValue,
      integrationByMenuId,
      isReadOnly,
      isSubmitting,
      mode,
      onSend,
      planReferenceByMenuId,
      sendDisabledReason,
      slashCommandByMenuId,
      skillByMenuId,
      value,
    ],
  );

  const prepareMessageForSend = useCallback(
    (
      message: string,
    ): { message: string; options?: AgentComposerSendOptions } => {
      const folderReferenceSnapshots = (
        conversationId ? folderReferences.data ?? [] : folders
      ).map(
        (reference): MessageFolderReference => ({
          ...(reference.id ? { id: reference.id } : {}),
          folderPath: reference.folderPath,
          displayName: reference.displayName,
        }),
      );
      const excerptReferences = normalizeComposerExcerptReferences(
        selectedExcerptReferenceList,
      );
      if (questionMode) {
        return {
          message,
          ...(folderReferenceSnapshots.length > 0 || excerptReferences.length > 0
            ? {
                options: {
                  ...(folderReferenceSnapshots.length > 0
                    ? { folderReferences: folderReferenceSnapshots }
                    : {}),
                  ...(excerptReferences.length > 0 ? { excerptReferences } : {}),
                },
              }
            : {}),
        };
      }
      const tokens = new Set(extractComposerSlashSkillTokens(message));
      const internalNames = new Set(selectedInternalSkillNames);
      for (const skill of skills) {
        if (skill.source === "ralphx-internal" && tokens.has(skill.name)) {
          internalNames.add(skill.invocationValue || skill.name);
        }
      }
      const withInternalSkillDirectives = appendInternalSkillDirectives(
        message,
        [...internalNames],
      );
      const references = new Map<string, AgentComposerProjectReference>();
      for (const reference of selectedProjectReferenceList) {
        references.set(reference.path, reference);
      }
      const projectReferences = normalizeComposerProjectReferences([
        ...references.values(),
      ]);
      const integrationReferences = new Map<
        string,
        AgentComposerIntegrationReference
      >();
      for (const reference of selectedIntegrationReferenceList) {
        integrationReferences.set(
          `${reference.provider}:${reference.kind}:${reference.id}`,
          reference,
        );
      }
      const normalizedIntegrationReferences =
        normalizeComposerIntegrationReferences([
          ...integrationReferences.values(),
        ]);
      const artifactReferences = new Map<
        string,
        AgentComposerArtifactReference
      >();
      for (const reference of selectedArtifactReferenceList) {
        artifactReferences.set(
          `${reference.kind}:${reference.artifactId}`,
          reference,
        );
      }
      for (const reference of extractComposerArtifactTokens(message)) {
        const key = `${reference.kind}:${reference.artifactId}`;
        if (!artifactReferences.has(key)) {
          artifactReferences.set(key, reference);
        }
      }
      const normalizedArtifactReferences = normalizeComposerArtifactReferences([
        ...artifactReferences.values(),
      ]);
      const capabilityIntent = capability
        ? ({ coordinationMode: capability.value } satisfies CapabilityIntent)
        : null;
      return {
        message: withInternalSkillDirectives,
        ...(folderReferenceSnapshots.length > 0 ||
          projectReferences.length > 0 ||
          normalizedIntegrationReferences.length > 0 ||
          normalizedArtifactReferences.length > 0 ||
          excerptReferences.length > 0 ||
          capabilityIntent
          ? {
              options: {
                ...(folderReferenceSnapshots.length > 0
                  ? { folderReferences: folderReferenceSnapshots }
                  : {}),
                ...(projectReferences.length > 0 ? { projectReferences } : {}),
                ...(normalizedIntegrationReferences.length > 0
                  ? { integrationReferences: normalizedIntegrationReferences }
                  : {}),
                ...(normalizedArtifactReferences.length > 0
                  ? { artifactReferences: normalizedArtifactReferences }
                  : {}),
                ...(excerptReferences.length > 0 ? { excerptReferences } : {}),
                ...(capabilityIntent ? { capabilityIntent } : {}),
              },
            }
          : {}),
      };
    },
    [
      conversationId,
      folderReferences.data,
      folders,
      questionMode,
      selectedArtifactReferenceList,
      selectedIntegrationReferenceList,
      selectedExcerptReferenceList,
      selectedInternalSkillNames,
      selectedProjectReferenceList,
      skills,
      capability,
    ],
  );

  const removeSelectedProjectReference = useCallback(
    (path: string) => {
      setSelectedProjectReferences((current) => {
        const nextSet = new Map(current);
        nextSet.delete(path);
        return nextSet;
      });
      focusTextareaAtComposerCursor(cursorPosition);
    },
    [cursorPosition, focusTextareaAtComposerCursor],
  );

  const removeSelectedIntegrationReference = useCallback(
    (reference: AgentComposerIntegrationReference) => {
      setSelectedIntegrationReferences((current) => {
        const nextSet = new Map(current);
        nextSet.delete(`${reference.kind}:${reference.id}`);
        nextSet.delete(`${reference.provider}:${reference.kind}:${reference.id}`);
        return nextSet;
      });
      focusTextareaAtComposerCursor(cursorPosition);
    },
    [cursorPosition, focusTextareaAtComposerCursor],
  );

  const removeSelectedArtifactReference = useCallback(
    (reference: AgentComposerArtifactReference) => {
      setSelectedArtifactReferences((current) => {
        const nextSet = new Map(current);
        nextSet.delete(`${reference.kind}:${reference.artifactId}`);
        return nextSet;
      });
      focusTextareaAtComposerCursor(cursorPosition);
    },
    [cursorPosition, focusTextareaAtComposerCursor],
  );

  const removeSelectedExcerptReference = useCallback(
    (reference: ComposerExcerptReference) => {
      setSelectedExcerptReferences((current) => {
        const next = new Map(current);
        next.delete(composerExcerptReferenceKey(reference));
        return next;
      });
      focusTextareaAtComposerCursor(cursorPosition);
    },
    [cursorPosition, focusTextareaAtComposerCursor],
  );

  const handleAttachmentSelect = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const fileList = event.target.files;
      if (!fileList || fileList.length === 0) {
        return;
      }

      const validFiles = validateChatAttachmentFiles(fileList, {
        maxFiles: CHAT_ATTACHMENT_MAX_FILES,
        maxFileSize: CHAT_ATTACHMENT_MAX_FILE_SIZE,
      });

      if (validFiles.length > 0) {
        void onFilesSelected?.(validFiles);
      }

      event.target.value = "";
    },
    [onFilesSelected],
  );

  const handleOpenAttachmentPicker = useCallback(() => {
    if (!attachmentDisabled) {
      fileInputRef.current?.click();
    }
  }, [attachmentDisabled]);

  const handleAddFolder = useCallback(async () => {
    if (attachmentDisabled) return;
    const selected = await openDialog({ directory: true, multiple: false });
    const folderPath = Array.isArray(selected) ? selected[0] : selected;
    if (!folderPath) return;
    const displayName =
      folderPath.split(/[\\/]/).filter(Boolean).pop() ?? folderPath;
    setFolderError(null);
    if (conversationId) {
      try {
        await addFolderReference.mutateAsync({
          conversationId,
          folderPath,
          displayName,
        });
      } catch (error) {
        setFolderError(
          extractErrorMessage(error, "Unable to add folder."),
        );
      }
      return;
    }
    onFoldersSelected?.([
      ...folders,
      {
        id:
          globalThis.crypto?.randomUUID?.() ??
          `${folderPath}-${Date.now()}`,
        folderPath,
        displayName,
      },
    ]);
  }, [
    addFolderReference,
    attachmentDisabled,
    conversationId,
    folders,
    onFoldersSelected,
  ]);

  useEffect(() => {
    if (!folderReferencesSupported || !conversationId) return;
    const frame = requestAnimationFrame(() => setTimeout(() => setFolderReferencesEnabled(true), 0));
    return () => {
      cancelAnimationFrame(frame);
      setFolderReferencesEnabled(false);
    };
  }, [conversationId, folderReferencesSupported]);

  const handleSend = useCallback(async () => {
    const trimmedValue = value.trim();
    const messageValue = trimmedValue || emptySubmitValue;
    if (!messageValue) {
      if (shouldShowStop) {
        await onStop?.();
      }
      return;
    }

    if ((isSubmitting && !canSendWhileAgentActive) || isReadOnly || sendDisabledReason) {
      return;
    }

    addHistoryEntry(messageValue);
    const outgoing = prepareMessageForSend(messageValue);

    const sendOutgoing = () =>
      outgoing.options
        ? onSend(outgoing.message, outgoing.options)
        : onSend(outgoing.message);

    if (questionMode || isControlled) {
      try {
        await sendOutgoing();
      } catch {
        return;
      }
      setSelectedInternalSkillNames(new Set());
      setSelectedProjectReferences(new Map());
      setSelectedIntegrationReferences(new Map());
      setSelectedArtifactReferences(new Map());
      setSelectedExcerptReferences(new Map());
      return;
    }

    clearValue({ preserveExcerptReferences: true });
    try {
      await sendOutgoing();
      setSelectedExcerptReferences(new Map());
    } catch {
      // Errors surface through the parent; preserve the current interaction model.
    }
  }, [
    addHistoryEntry,
    canSendWhileAgentActive,
    clearValue,
    emptySubmitValue,
    isControlled,
    isReadOnly,
    isSubmitting,
    onSend,
    onStop,
    prepareMessageForSend,
    questionMode,
    sendDisabledReason,
    shouldShowStop,
    value,
  ]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (shouldShowCommandMenu && activeTrigger) {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          setActiveMenuIndex((index) =>
            menuItems.length > 0 ? (index + 1) % menuItems.length : 0,
          );
          return;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          setActiveMenuIndex((index) =>
            menuItems.length > 0
              ? (index - 1 + menuItems.length) % menuItems.length
              : 0,
          );
          return;
        }
        if (
          (event.key === "Enter" || event.key === "Tab") &&
          menuItems.length > 0
        ) {
          event.preventDefault();
          selectMenuItem(
            menuItems[Math.min(activeMenuIndex, menuItems.length - 1)]!,
          );
          return;
        }
      }

      if (event.key === "Escape") {
        event.preventDefault();
        (event.target as HTMLTextAreaElement).blur();
        return;
      }

      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        void handleSend();
        return;
      }

      if (event.key === "ArrowUp" && !value && hasQueuedMessages) {
        event.preventDefault();
        onEditLastQueued?.();
        return;
      }

      if (handleHistoryKeyDown(event, value)) {
        return;
      }
    },
    [
      activeMenuIndex,
      activeTrigger,
      handleHistoryKeyDown,
      handleSend,
      hasQueuedMessages,
      menuItems,
      onEditLastQueued,
      selectMenuItem,
      shouldShowCommandMenu,
      value,
    ],
  );

  const helperText = useMemo(() => {
    if (!showHelperText) {
      return null;
    }
    return (
      <div
        className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.625rem] font-medium"
        style={{ color: "var(--text-muted)" }}
      >
        <span>Press Enter to send</span>
        <span aria-hidden="true" style={{ color: "var(--overlay-moderate)" }}>
          •
        </span>
        <span>&#x21E7; Enter for a new line</span>
        <span aria-hidden="true" style={{ color: "var(--overlay-moderate)" }}>
          •
        </span>
        <span>Type / for commands and skills</span>
        <span aria-hidden="true" style={{ color: "var(--overlay-moderate)" }}>
          •
        </span>
        <span>@ for references</span>
      </div>
    );
  }, [showHelperText]);

  const updateCursorFromTextarea = useCallback(
    (textarea: HTMLTextAreaElement) => {
      setCursorPosition(textarea.selectionStart ?? textarea.value.length);
    },
    [],
  );

  const handleTextareaChange = useCallback(
    (event: React.ChangeEvent<HTMLTextAreaElement>) => {
      setValue(event.target.value);
      updateCursorFromTextarea(event.target);
    },
    [setValue, updateCursorFromTextarea],
  );

  const resolvePastedAtlassianUrls = useCallback(
    async (urls: readonly string[]) => {
      try {
        const results = await atlassianApi.resolveResourceUrls({
          urls: [...urls],
        });
        const resolvedUrls: string[] = [];
        const references: AgentComposerIntegrationReference[] = [];
        for (const result of results) {
          if (!result.resource) {
            continue;
          }
          resolvedUrls.push(result.inputUrl.trim());
          references.push(
            atlassianResourceToIntegrationReference(result.resource),
          );
        }
        if (references.length === 0) {
          return;
        }
        addSelectedIntegrationReferences(references);
        const currentValue = latestValueRef.current;
        const nextValue = removeResolvedAtlassianResourceUrls(
          currentValue,
          resolvedUrls,
        );
        if (nextValue === currentValue) {
          return;
        }
        const nextCursor = Math.min(
          textareaRef.current?.selectionStart ?? nextValue.length,
          nextValue.length,
        );
        setValue(nextValue);
        setCursorPosition(nextCursor);
      } catch {
        // Disabled, invalid, or inaccessible integrations leave pasted text intact.
      }
    },
    [addSelectedIntegrationReferences, setValue],
  );

  const handleTextareaPaste = useCallback(
    (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
      if (questionMode || isReadOnly || (isSubmitting && !canSendWhileAgentActive)) {
        return;
      }
      const pastedText = event.clipboardData.getData("text");
      const urls = extractPastedAtlassianResourceUrls(pastedText);
      if (urls.length === 0) {
        return;
      }
      event.preventDefault();
      const textarea = event.currentTarget;
      const selectionStart = textarea.selectionStart ?? value.length;
      const selectionEnd = textarea.selectionEnd ?? selectionStart;
      const nextValue = `${value.slice(0, selectionStart)}${pastedText}${value.slice(selectionEnd)}`;
      const nextCursor = selectionStart + pastedText.length;
      setValue(nextValue);
      setCursorPosition(nextCursor);
      restoreTextareaFocusCursorRef.current = nextCursor;
      window.requestAnimationFrame(() => {
        window.setTimeout(() => {
          void resolvePastedAtlassianUrls(urls);
        }, 0);
      });
    },
    [
      canSendWhileAgentActive,
      isReadOnly,
      isSubmitting,
      questionMode,
      resolvePastedAtlassianUrls,
      setValue,
      value,
    ],
  );

  const attachmentDropEnabled =
    enableAttachments && !attachmentDisabled && onFilesSelected !== undefined;
  const { isDragging: isAttachmentDragging, dropProps: attachmentDropProps } =
    useChatAttachmentDrop({
      enabled: attachmentDropEnabled,
      targetRef: surfaceRef,
      onFilesSelected,
      maxFiles: CHAT_ATTACHMENT_MAX_FILES,
      maxFileSize: CHAT_ATTACHMENT_MAX_FILE_SIZE,
    });

  return (
    <div
      ref={surfaceRef}
      data-testid={dataTestId}
      data-collapsible={collapsible ? "true" : "false"}
      data-collapsed={isCollapsed ? "true" : "false"}
      className={cn(
        "agent-composer-surface relative mx-auto w-full max-w-full",
        className,
      )}
      {...attachmentDropProps}
    >
      <div
        className="overflow-hidden rounded-[22px] border transition-colors"
        style={{
          background: "var(--bg-surface)",
          borderColor: isFocused
            ? "var(--accent-border)"
            : "var(--form-border)",
          boxShadow: "var(--shadow-sm)",
        }}
      >
        {shouldShowCommandMenu && (
          <AgentComposerCommandMenu
            items={menuItems}
            activeIndex={Math.min(
              activeMenuIndex,
              Math.max(menuItems.length - 1, 0),
            )}
            onActiveIndexChange={setActiveMenuIndex}
            onSelect={selectMenuItem}
            isLoading={menuLoading}
            emptyLabel={menuEmptyLabel}
          />
        )}
        <textarea
          ref={textareaRef}
          data-testid={textareaTestId}
          value={value}
          onChange={handleTextareaChange}
          onPaste={handleTextareaPaste}
          onKeyDown={handleKeyDown}
          onKeyUp={(event) => updateCursorFromTextarea(event.currentTarget)}
          onClick={(event) => updateCursorFromTextarea(event.currentTarget)}
          onSelect={(event) => updateCursorFromTextarea(event.currentTarget)}
          onFocus={(event) => {
            setIsFocused(true);
            updateCursorFromTextarea(event.currentTarget);
            onFocusChange?.(true);
          }}
          onBlur={() => {
            setIsFocused(false);
            onFocusChange?.(false);
          }}
          disabled={isReadOnly || (isSubmitting && !canSendWhileAgentActive)}
          placeholder={effectivePlaceholder}
          className={cn(
            "agent-composer-textarea block w-full resize-none border-0 bg-transparent px-5 text-[0.9375rem] leading-[1.5] shadow-none outline-none ring-0 focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0 sm:text-[1rem]",
            collapsible && "transition-[height] duration-150 ease-out",
            compact ? "pb-2 pt-2" : "pb-2 pt-4",
          )}
          style={{
            color: "var(--text-primary)",
            boxShadow: "none",
            outline: "none",
            WebkitAppearance: "none",
            appearance: "none",
          }}
          aria-label="Message input"
        />

        {(attachments.length > 0 || folders.length > 0 || folderReferences.data?.length ||
          folderReferences.isError ||
          hasSelectedReferences ||
          attachmentsUploading) && (
          <div className="px-5 pb-3">
            {folderReferences.isError && (
              <div
                className="mb-3 flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-xs"
                style={{
                  borderColor: "var(--status-warning-border)",
                  backgroundColor: "var(--status-warning-muted)",
                  color: "var(--status-warning)",
                }}
              >
                <span>
                  Couldn't load folder references — previously attached folders may still be visible to the agent
                </span>
                <button
                  type="button"
                  className="shrink-0 font-medium underline underline-offset-2"
                  aria-label="Retry folder references"
                  onClick={() => void folderReferences.refetch()}
                >
                  Retry
                </button>
              </div>
            )}
            {folderReferences.data && (
              <FolderReferenceChips
                references={folderReferences.data}
                {...(removeFolderReference.isPending &&
                removeFolderReference.variables?.folderReferenceId
                  ? { removingId: removeFolderReference.variables.folderReferenceId }
                  : {})}
                onRemove={(reference) => {
                  void removeFolderReference.mutateAsync({
                    conversationId: reference.conversationId,
                    folderReferenceId: reference.id,
                  }).catch((error: unknown) => setFolderError(extractErrorMessage(error, "Unable to remove folder.")));
                }}
              />
            )}
            {folders.length > 0 && (
              <FolderReferenceChips
                references={folders}
                testId="draft-folder-reference-chips"
                onRemove={(folder) => onRemoveFolder?.(folder.id)}
              />
            )}
            {attachments.length > 0 && (
              <div className="pb-3">
                <ChatAttachmentGallery
                  attachments={attachments}
                  {...(onRemoveAttachment
                    ? { onRemove: onRemoveAttachment }
                    : {})}
                  uploading={attachmentsUploading}
                  compact
                />
              </div>
            )}
            {hasSelectedReferences && (
              <div className="pb-3">
                <ComposerReferencePills
                  projectReferences={selectedProjectReferenceList}
                  integrationReferences={selectedIntegrationReferenceList}
                  artifactReferences={selectedArtifactReferenceList}
                  excerptReferences={selectedExcerptReferenceList}
                  onRemoveProjectReference={removeSelectedProjectReference}
                  onRemoveIntegrationReference={
                    removeSelectedIntegrationReference
                  }
                  onRemoveArtifactReference={removeSelectedArtifactReference}
                  onRemoveExcerptReference={removeSelectedExcerptReference}
                />
              </div>
            )}
          </div>
        )}

        {/* Keyboard helper stays mounted but reveals only when active so the
            resting composer reclaims that vertical space (collapsible hosts). */}
        {helperText && (
          <div
            data-testid="agent-composer-helper-reveal"
            data-visible={isExpanded ? "true" : "false"}
            aria-hidden={isExpanded ? undefined : true}
            className={cn(
              "overflow-hidden px-5 transition-all duration-150 ease-out",
              isExpanded
                ? "max-h-20 pb-3 opacity-100"
                : "pointer-events-none max-h-0 pb-0 opacity-0",
            )}
          >
            {helperText}
          </div>
        )}
        {folderError && <p className="px-5 pb-3 text-xs" role="alert" style={{ color: "var(--status-error)" }}>{folderError}</p>}

        <div
          className={cn(
            "border-t px-3.5 transition-[padding] duration-150 ease-out",
            compact ? "py-1.5" : "py-2",
          )}
          style={{
            borderColor: "var(--overlay-faint)",
            background:
              "color-mix(in srgb, var(--bg-base) 16%, var(--bg-surface) 84%)",
          }}
        >
          <div
            className={cn(
              "agent-composer-control-row flex flex-wrap items-center transition-[gap] duration-150 ease-out",
              compact ? "gap-1.5" : "gap-2",
            )}
          >
            {enableAttachments && (
              <input
                ref={fileInputRef}
                data-testid="attachment-file-input"
                type="file"
                multiple
                accept={CHAT_ATTACHMENT_ACCEPTED_TYPES}
                onChange={handleAttachmentSelect}
                className="hidden"
                aria-hidden="true"
                tabIndex={-1}
              />
            )}

            <ComposerActionMenu
              enableAttachments={enableAttachments}
              attachmentDisabled={attachmentDisabled}
              onOpenAttachmentPicker={handleOpenAttachmentPicker}
              showAddFolder={folderReferencesSupported}
              onAddFolder={handleAddFolder}
              {...(onForkSession
                ? {
                    onForkSession,
                    forkSessionDisabled:
                      forkSessionDisabled ||
                      isReadOnly ||
                      (isSubmitting && !canSendWhileAgentActive),
                  }
                : {})}
              open={actionMenuOpen}
              onOpenChange={setActionMenuOpen}
              integrationAvailability={integrationAvailability}
              onInsertIntegrationTrigger={(kind) => {
                restoreTextareaFocusOnActionMenuCloseRef.current = true;
                markComposerFocused();
                insertIntegrationTrigger(
                  kind,
                  value,
                  cursorPosition,
                  applyComposerText,
                  textareaRef.current,
                );
                setActionMenuOpen(false);
              }}
              onInsertPlanTrigger={() => {
                restoreTextareaFocusOnActionMenuCloseRef.current = true;
                markComposerFocused();
                insertPlanTrigger(
                  value,
                  cursorPosition,
                  applyComposerText,
                  textareaRef.current,
                );
                setActionMenuOpen(false);
              }}
              onCloseAutoFocus={(event) => {
                if (!restoreTextareaFocusOnActionMenuCloseRef.current) {
                  return;
                }
                restoreTextareaFocusOnActionMenuCloseRef.current = false;
                event.preventDefault();
                focusTextareaAtComposerCursor(cursorPosition);
              }}
              compact={compact}
            />

            {/* Control order per product direction: mode → model → chat focus.
                The chat-focus (workspace) selector sits inline to the right of
                the model/runtime pill rather than wrapping to its own row. */}
            {mode && (
              <ComposerModeChip
                mode={mode}
                open={modeMenuOpen}
                onOpenChange={setModeMenuOpen}
                compact={compact}
              />
            )}

            <div className="flex min-w-0 flex-[0_1_auto] items-stretch gap-2">
              <ComposerRuntimeSelector
                provider={provider}
                model={model}
                effort={effort}
                {...(capability ? { capability } : {})}
                {...(runtimePersona ? { persona: runtimePersona } : {})}
                {...(speed ? { speed } : {})}
                {...(runtimeDefault ? { runtimeDefault } : {})}
                compact={compact}
                className="max-w-[34rem]"
                surfaceRef={surfaceRef}
              />
            </div>

            {chatFocus && chatFocus.options.length > 1 && (
              <div className="agent-composer-chat-focus-slot flex min-w-0 shrink-0">
                <ComposerChatFocusPill
                  chatFocus={chatFocus}
                  compact={compact}
                />
              </div>
            )}

            <Button
              type="button"
              className={cn(
                "agent-composer-action-button shrink-0 rounded-full text-[0.75rem] font-semibold tracking-[-0.01em] transition-[height,min-width,padding] duration-150 ease-out",
                "ml-auto",
                compact ? "h-8 px-3" : "h-10 px-4",
                compact
                  ? "min-w-0"
                  : shouldShowStop
                    ? "min-w-[100px]"
                    : "min-w-[118px]",
              )}
              style={{
                background:
                  shouldShowStop || canSubmit
                    ? "var(--accent-primary)"
                    : withAlpha("var(--accent-primary)", 40),
                color: "var(--text-on-accent)",
                boxShadow: "none",
              }}
              onClick={() => {
                if (shouldShowStop) {
                  void onStop?.();
                  return;
                }
                void handleSend();
              }}
              disabled={shouldShowStop ? false : !canSubmit}
              data-testid={actionTestId}
              aria-label={shouldShowStop ? "Stop agent" : submitLabel}
            >
              {shouldShowStop ? (
                <>
                  <Square className="h-3.5 w-3.5 fill-current" />
                  <span className="agent-composer-action-label">Stop</span>
                </>
              ) : isSubmitting && !canSendWhileAgentActive ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span className="agent-composer-action-label">
                    {submittingLabel}
                  </span>
                </>
              ) : (
                <>
                  <ArrowUp className="h-4 w-4" />
                  <span className="agent-composer-action-label">
                    {submitLabel}
                  </span>
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
      {isAttachmentDragging && (
        <ChatAttachmentDropOverlay roundedClassName="rounded-[22px]" />
      )}
    </div>
  );
}

function ComposerActionMenu({
  enableAttachments,
  attachmentDisabled,
  onOpenAttachmentPicker,
  showAddFolder = false,
  onAddFolder,
  onForkSession,
  forkSessionDisabled = false,
  open,
  onOpenChange,
  onInsertIntegrationTrigger,
  integrationAvailability,
  onInsertPlanTrigger,
  onCloseAutoFocus,
  compact = false,
}: {
  enableAttachments: boolean;
  attachmentDisabled: boolean;
  onOpenAttachmentPicker: () => void;
  showAddFolder?: boolean;
  onAddFolder?: () => void;
  onForkSession?: (() => Promise<unknown> | void) | undefined;
  forkSessionDisabled?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onInsertIntegrationTrigger: (kind: AgentComposerIntegrationKind) => void;
  integrationAvailability: AgentComposerIntegrationAvailability;
  onInsertPlanTrigger: () => void;
  onCloseAutoFocus?: (event: Event) => void;
  compact?: boolean;
}) {
  const availableIntegrationActions = COMPOSER_INTEGRATION_ACTIONS.filter(
    ({ kind }) => integrationAvailability[kind],
  );
  const hasPersistentActions = true;
  const hasPrimaryActions =
    enableAttachments ||
    showAddFolder ||
    Boolean(onForkSession);
  const setOpen = onOpenChange;

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            "agent-composer-plus-trigger flex shrink-0 items-center justify-center rounded-md transition-[height,width,background-color,color] duration-150 ease-out disabled:opacity-40",
            compact ? "h-8 w-8" : "h-10 w-10",
            !hasPersistentActions && "agent-composer-compact-only",
          )}
          style={{
            background:
              "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
            color: "var(--text-secondary)",
            border: "1px solid var(--form-border)",
            boxShadow: "none",
          }}
          aria-label="Open composer actions"
          data-testid="agent-composer-actions-menu"
        >
          <Plus className="h-4 w-4" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        side="top"
        sideOffset={8}
        className="w-64 rounded-xl p-1.5"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          color: "var(--text-primary)",
        }}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={onCloseAutoFocus}
      >
        {enableAttachments && (
          <button
            type="button"
            disabled={attachmentDisabled}
            className="flex h-10 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors disabled:opacity-50"
            style={{ color: "var(--text-primary)" }}
            onClick={() => {
              onOpenAttachmentPicker();
              setOpen(false);
            }}
          >
            <Paperclip className="h-4 w-4" />
            Add files
          </button>
        )}

        {showAddFolder && (
          <button
            type="button"
            disabled={attachmentDisabled}
            className="flex h-10 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors disabled:opacity-50"
            style={{ color: "var(--text-primary)" }}
            onClick={() => {
              onAddFolder?.();
              setOpen(false);
            }}
          >
            <FolderOpen className="h-4 w-4" />
            Add folder
          </button>
        )}

        {onForkSession && (
          <>
            {(enableAttachments || showAddFolder) && (
              <div
                className="my-1 h-px"
                style={{ background: "var(--overlay-weak)" }}
              />
            )}
            <button
              type="button"
              disabled={forkSessionDisabled}
              className="flex h-10 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)] disabled:opacity-50"
              style={{ color: "var(--text-primary)" }}
              onClick={() => {
                setOpen(false);
                void Promise.resolve(onForkSession()).catch(() => {
                  // Parent action handlers surface their own errors.
                });
              }}
            >
              <GitFork className="h-4 w-4" />
              Fork session
            </button>
          </>
        )}

        {hasPrimaryActions && (
          <div
            className="my-1 h-px"
            style={{ background: "var(--overlay-weak)" }}
          />
        )}
        <div className="py-1">
          <div className="px-2 py-1 text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
            References
          </div>
          <button
            type="button"
            className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
            onClick={onInsertPlanTrigger}
          >
            <ScrollText className="h-4 w-4" />
            Plan
          </button>
        </div>
        {availableIntegrationActions.length > 0 && (
          <>
            <div
              className="my-1 h-px"
              style={{ background: "var(--overlay-weak)" }}
            />
            <div className="py-1">
              <div className="px-2 py-1 text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
                Integrations
              </div>
              <div className="space-y-1">
                {availableIntegrationActions.map(({ kind, label }) => (
                  <button
                    key={kind}
                    type="button"
                    className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
                    onClick={() => onInsertIntegrationTrigger(kind)}
                  >
                    <Search className="h-4 w-4" />
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

function ComposerReferencePills({
  projectReferences,
  integrationReferences,
  artifactReferences,
  excerptReferences,
  onRemoveProjectReference,
  onRemoveIntegrationReference,
  onRemoveArtifactReference,
  onRemoveExcerptReference,
}: {
  projectReferences: AgentComposerProjectReference[];
  integrationReferences: AgentComposerIntegrationReference[];
  artifactReferences: AgentComposerArtifactReference[];
  excerptReferences: ComposerExcerptReference[];
  onRemoveProjectReference: (path: string) => void;
  onRemoveIntegrationReference: (
    reference: AgentComposerIntegrationReference,
  ) => void;
  onRemoveArtifactReference: (
    reference: AgentComposerArtifactReference,
  ) => void;
  onRemoveExcerptReference: (reference: ComposerExcerptReference) => void;
}) {
  if (
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0 &&
    excerptReferences.length === 0
  ) {
    return null;
  }

  return (
    <div
      data-testid="agent-composer-reference-pills"
      className="flex flex-wrap gap-2"
    >
      {projectReferences.map((reference) => {
        const isFolder = reference.kind === "directory";
        return (
          <ComposerReferencePill
            key={`project:${reference.path}`}
            testId={`agent-composer-reference-pill-project:${reference.path}`}
            icon={isFolder ? FolderOpen : FileText}
            typeLabel={isFolder ? "Folder" : "File"}
            label={reference.path}
            removeLabel={`Remove ${isFolder ? "folder" : "file"} reference ${reference.path}`}
            onRemove={() => onRemoveProjectReference(reference.path)}
          />
        );
      })}
      {integrationReferences.map((reference) => {
        const isJira = reference.kind === "jira";
        const isLinear = reference.kind === "linear";
        const isClickUp = reference.kind === "clickup";
        const isGranola = reference.provider === "granola" && reference.kind === "note";
        const label =
          isJira || isLinear || isClickUp
            ? (reference.key ?? reference.id)
            : (reference.title ?? reference.id);
        const description =
          isJira || isLinear || isClickUp
            ? reference.title
            : isGranola
              ? reference.summaryExcerpt
              : reference.id;
        const typeLabel = isClickUp
          ? "ClickUp"
          : isLinear
            ? "Linear"
            : isJira
              ? "Jira"
              : isGranola
                ? "Granola"
                : "Confluence";
        return (
          <ComposerReferencePill
            key={`integration:${reference.provider}:${reference.kind}:${reference.id}`}
            testId={`agent-composer-reference-pill-integration:${reference.kind}:${reference.id}`}
            icon={isJira || isLinear || isClickUp ? Ticket : isGranola ? ScrollText : BookOpen}
            typeLabel={typeLabel}
            label={label}
            removeLabel={`Remove ${typeLabel} reference ${label}`}
            onRemove={() => onRemoveIntegrationReference(reference)}
            {...(description ? { description } : {})}
          />
        );
      })}
      {artifactReferences.map((reference) => {
        const label = reference.title ?? shortReferenceId(reference.artifactId);
        const description = [
          reference.status ? formatPlanReferenceStatus(reference.status) : null,
          reference.version ? `v${reference.version}` : null,
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          <ComposerReferencePill
            key={`artifact:${reference.kind}:${reference.artifactId}`}
            testId={`agent-composer-reference-pill-artifact:${reference.kind}:${reference.artifactId}`}
            icon={ScrollText}
            typeLabel={reference.kind === "plan" ? "Plan" : "Artifact"}
            label={label}
            removeLabel={`Remove ${reference.kind === "plan" ? "plan" : "artifact"} reference ${label}`}
            onRemove={() => onRemoveArtifactReference(reference)}
            {...(description ? { description } : {})}
          />
        );
      })}
      {excerptReferences.map((reference) => {
        const label = reference.title ?? reference.sourceId;
        const description = [
          reference.version !== undefined ? `v${reference.version}` : null,
          reference.filePath,
          reference.locator,
          reference.excerpt,
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          <ComposerReferencePill
            key={composerExcerptReferenceKey(reference)}
            testId={`agent-composer-reference-pill-excerpt:${reference.sourceKind}:${reference.sourceId}`}
            icon={ScrollText}
            typeLabel={`${reference.sourceLabel} excerpt`}
            label={label}
            description={description}
            removeLabel={`Remove ${reference.sourceLabel} excerpt ${label}`}
            onRemove={() => onRemoveExcerptReference(reference)}
          />
        );
      })}
    </div>
  );
}

function insertIntegrationTrigger(
  kind: AgentComposerIntegrationKind,
  value: string,
  cursorPosition: number,
  applyComposerText: (nextValue: string, nextCursor: number) => void,
  textarea: HTMLTextAreaElement | null,
) {
  const start = textarea?.selectionStart ?? cursorPosition;
  const end = textarea?.selectionEnd ?? cursorPosition;
  const trigger =
    kind === "jira"
      ? "@jira:"
      : kind === "linear"
        ? "@linear:"
      : kind === "clickup"
        ? "@clickup:"
        : kind === "granola"
          ? "@granola:"
          : "@confluence:";
  const before = value.slice(0, start);
  const after = value.slice(end);
  const spacer = before.length > 0 && !/\s$/.test(before) ? " " : "";
  const nextText = `${before}${spacer}${trigger}${after}`;
  applyComposerText(nextText, before.length + spacer.length + trigger.length);
}

function insertPlanTrigger(
  value: string,
  cursorPosition: number,
  applyComposerText: (nextValue: string, nextCursor: number) => void,
  textarea: HTMLTextAreaElement | null,
) {
  const start = textarea?.selectionStart ?? cursorPosition;
  const end = textarea?.selectionEnd ?? cursorPosition;
  const trigger = "@plan:";
  const before = value.slice(0, start);
  const after = value.slice(end);
  const spacer = before.length > 0 && !/\s$/.test(before) ? " " : "";
  const nextText = `${before}${spacer}${trigger}${after}`;
  applyComposerText(nextText, before.length + spacer.length + trigger.length);
}

function formatPlanReferenceStatus(status: string): string {
  if (status === "approved") {
    return "Approved";
  }
  if (status === "accepted") {
    return "Accepted";
  }
  return "Draft";
}

function shortReferenceId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}...` : id;
}

function ComposerModeChip({
  mode,
  open,
  onOpenChange,
  compact = false,
}: {
  mode: ModeFieldConfig;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  compact?: boolean;
}) {
  const activeOption = mode.options.find((o) => o.id === mode.value);
  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (next) {
        void mode.onOpen?.();
      }
      onOpenChange(next);
    },
    [mode, onOpenChange],
  );
  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <button
          type="button"
          disabled={mode.disabled}
          data-testid={
            mode.testId ? `${mode.testId}-chip` : "agent-composer-mode-chip"
          }
          data-composer-mode-chip="true"
          aria-label={`Mode: ${activeOption?.label ?? mode.value}. Click to change.`}
          className={cn(
            "inline-flex shrink-0 items-center gap-2 rounded-md border transition-[height,padding,background-color] duration-150 ease-out hover:bg-[var(--bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed",
            compact ? "h-8 px-2.5" : "h-10 px-3",
          )}
          style={{
            background:
              "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
            borderColor: "var(--form-border)",
          }}
        >
          {/* Eyebrow label only in the expanded state; the mini/resting composer
              shows just the value to stay minimal. */}
          {!compact && (
            <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
              Mode
            </span>
          )}
          <span className="text-[0.8125rem] font-medium text-[var(--text-primary)]">
            {activeOption?.label ?? "—"}
          </span>
        </button>
      </PopoverTrigger>
      {/* The mode chip owns its own popover with ONLY the workflow modes; the
          "+" action menu carries everything else (attachments, references…). */}
      <PopoverContent
        align="start"
        side="top"
        sideOffset={8}
        className="w-56 rounded-xl p-1.5"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          color: "var(--text-primary)",
        }}
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        <ComposerModeMenuSection
          mode={mode}
          onDone={() => onOpenChange(false)}
        />
      </PopoverContent>
    </Popover>
  );
}

function ComposerChatFocusPill({
  chatFocus,
  compact = false,
}: {
  chatFocus: ChatFocusFieldConfig;
  compact?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const activeOption =
    chatFocus.options.find((o) => o.id === chatFocus.value) ??
    chatFocus.options[0];
  const ActiveIcon = activeOption?.icon;
  const triggerStyle = activeOption?.toneColor
    ? {
        background: activeOption.toneBackground ?? "var(--bg-surface)",
        borderColor: activeOption.toneBorder ?? "var(--form-border)",
        color: activeOption.toneColor,
      }
    : {
        background:
          "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
        borderColor: "var(--form-border)",
        color: "var(--text-primary)",
      };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          disabled={chatFocus.disabled}
          data-testid={
            chatFocus.testId
              ? `${chatFocus.testId}-pill`
              : "agent-composer-chat-focus-pill"
          }
          aria-label={`Chat focus: ${activeOption?.label ?? chatFocus.value}. Click to change.`}
          className={cn(
            "flex min-w-0 shrink-0 items-center gap-2 rounded-md border transition-[height,padding,background-color] duration-150 ease-out disabled:opacity-50",
            compact ? "h-8 px-2.5" : "h-10 px-3",
          )}
          style={triggerStyle}
        >
          {/* Eyebrow label only in the expanded state (matches the Mode chip);
              the mini/resting composer relies on the icon + value. */}
          {!compact && (
            <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
              Chat
            </span>
          )}
          <span className="flex min-w-0 items-center gap-1.5 text-[0.8125rem] font-medium">
            {ActiveIcon ? <ActiveIcon className="h-3.5 w-3.5" /> : null}
            <span className="truncate">{activeOption?.label ?? "—"}</span>
          </span>
          <ChevronDown className="h-3.5 w-3.5 opacity-70" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={6}
        className="w-56 rounded-xl p-1"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        {chatFocus.options.map((option) => {
          const selected = option.id === chatFocus.value;
          const Icon = option.icon;
          const optionStyle =
            selected && option.toneColor
              ? {
                  color: option.toneColor,
                  background: option.toneBackground ?? "transparent",
                }
              : selected
                ? {
                    color: "var(--text-primary)",
                    background: "var(--bg-surface)",
                  }
                : {
                    color: "var(--text-secondary)",
                    background: "transparent",
                  };
          return (
            <button
              key={option.id}
              type="button"
              data-testid={
                chatFocus.testId
                  ? `${chatFocus.testId}-option-${option.id}`
                  : undefined
              }
              data-active={selected ? "true" : "false"}
              className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-[0.75rem] font-medium transition-colors"
              style={optionStyle}
              onMouseEnter={(e) => {
                if (!selected) {
                  e.currentTarget.style.background = "var(--overlay-faint)";
                }
              }}
              onMouseLeave={(e) => {
                if (!selected) {
                  e.currentTarget.style.background = "transparent";
                }
              }}
              onClick={() => {
                chatFocus.onValueChange(option.id);
                setOpen(false);
              }}
            >
              {Icon ? <Icon className="h-3.5 w-3.5 shrink-0" /> : null}
              <span>{option.label}</span>
            </button>
          );
        })}
      </PopoverContent>
    </Popover>
  );
}

function ComposerModeMenuSection({
  mode,
  onDone,
}: {
  mode: ModeFieldConfig;
  onDone: () => void;
}) {
  const secondaryOptionIds = useMemo(
    () => new Set(mode.secondaryOptionIds ?? []),
    [mode.secondaryOptionIds],
  );
  const [showSecondaryModes, setShowSecondaryModes] = useState(false);
  const hasSecondaryModes = mode.options.some((option) =>
    secondaryOptionIds.has(option.id),
  );
  const visibleOptions = mode.options.filter(
    (option) =>
      !secondaryOptionIds.has(option.id) ||
      showSecondaryModes ||
      option.id === mode.value,
  );

  return (
    <div className="py-1">
      <div className="px-2 py-1 text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
        Mode
      </div>
      <div className="space-y-1">
        {visibleOptions.map((option) => {
          const isSelected = option.id === mode.value;
          const optionDisabled = mode.disabled || option.disabled;
          return (
            <button
              key={option.id}
              type="button"
              disabled={optionDisabled}
              data-testid={
                mode.testId ? `${mode.testId}-${option.id}` : undefined
              }
              className={cn(
                "flex w-full items-start gap-2 rounded-lg px-2 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                isSelected
                  ? "bg-[var(--accent-muted)]"
                  : "hover:bg-[var(--bg-hover)]",
              )}
              onClick={() => {
                if (optionDisabled) {
                  return;
                }
                mode.onValueChange(option.id);
                onDone();
              }}
            >
              <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
                {isSelected && (
                  <Check className="h-4 w-4 text-[var(--accent-primary)]" />
                )}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-[0.8125rem] font-medium text-[var(--text-primary)]">
                  {option.label}
                </span>
                {option.description && (
                  <span className="mt-0.5 block text-[0.6875rem] leading-snug text-[var(--text-muted)]">
                    {option.description}
                  </span>
                )}
                {option.disabledReason && (
                  <span className="mt-1 block text-[0.6875rem] leading-snug text-[var(--text-muted)]">
                    {option.disabledReason}
                  </span>
                )}
              </span>
            </button>
          );
        })}
      </div>
      {hasSecondaryModes && (
        <button
          type="button"
          aria-expanded={showSecondaryModes}
          aria-label={
            showSecondaryModes ? "Show fewer modes" : "Show more modes"
          }
          className="mt-1 flex w-full items-center justify-center gap-1.5 border-t border-[var(--border-subtle)] px-2 pt-2 text-[0.6875rem] font-medium text-[var(--text-muted)] transition-colors hover:text-[var(--text-primary)]"
          onClick={() => setShowSecondaryModes((visible) => !visible)}
        >
          <ChevronDown
            aria-hidden="true"
            className={cn(
              "h-3.5 w-3.5 transition-transform",
              showSecondaryModes && "rotate-180",
            )}
          />
          {showSecondaryModes ? "Show less" : "Show more"}
        </button>
      )}
    </div>
  );
}

export function AgentComposerProjectLine({
  value,
  onValueChange,
  options,
  placeholder,
  disabled = false,
  testId,
  allowNoProject = false,
  standaloneCaption,
}: ProjectFieldConfig) {
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const selectedProject = options.find((option) => option.id === value) ?? null;
  const isStandalone = value === null;
  const filteredOptions = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) {
      return options;
    }
    return options.filter(
      (option) =>
        option.label.toLowerCase().includes(query) ||
        option.description?.toLowerCase().includes(query),
    );
  }, [options, searchQuery]);

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (!nextOpen) {
      setSearchQuery("");
    }
  };

  const trigger = (
    <button
      type="button"
      className={cn(
        "flex min-w-0 max-w-[min(100%,430px)] items-center gap-2 rounded-full px-2 py-1 text-[0.75rem] transition-colors",
        !disabled && "hover:bg-[var(--bg-hover)]",
        "disabled:cursor-not-allowed disabled:opacity-60",
      )}
      style={{ color: "var(--text-secondary)" }}
      disabled={disabled}
      data-testid={testId}
      data-theme-button-skip="true"
      aria-label="Project"
    >
      {isStandalone ? (
        <CircleOff className="h-3.5 w-3.5 shrink-0" />
      ) : (
        <FolderOpen className="h-3.5 w-3.5 shrink-0" />
      )}
      <span className="shrink-0 text-[0.625rem] font-medium uppercase tracking-[0.14em]">
        Project
      </span>
      <span
        className="min-w-0 truncate font-medium"
        style={{
          color: selectedProject
            ? "var(--text-primary)"
            : "var(--text-secondary)",
        }}
      >
        {isStandalone ? "No project" : (selectedProject?.label ?? placeholder)}
      </span>
      {!disabled && <ChevronDown className="h-3.5 w-3.5 shrink-0" />}
    </button>
  );

  if (disabled) {
    return (
      <div className="flex min-w-0 items-center gap-2">
        {trigger}
        {isStandalone && standaloneCaption && (
          <span className="shrink-0 text-[0.6875rem] text-[var(--text-muted)]">
            {standaloneCaption}
          </span>
        )}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 items-center gap-2">
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-[min(420px,calc(100vw-2rem))] p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <div className="border-b border-[var(--border-subtle)] p-2">
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder="Search projects..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              className="h-8 border-[var(--border-subtle)] bg-[var(--bg-surface)] pl-8 pr-2 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
            />
          </div>
        </div>
        <div className="max-h-72 overflow-y-auto overscroll-contain">
          <div className="p-1">
            {filteredOptions.length === 0 ? (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                No projects found
              </div>
            ) : (
              <div className="space-y-0.5">
                {filteredOptions.map((option) => {
                  const isSelected = option.id === value;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      className={cn(
                        "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
                        isSelected
                          ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]",
                      )}
                      onClick={() => {
                        onValueChange(option.id);
                        setOpen(false);
                        setSearchQuery("");
                      }}
                    >
                      <span className="mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                        {isSelected && <Check className="h-3.5 w-3.5" />}
                      </span>
                      <span className="min-w-0">
                        <span className="block whitespace-normal break-words font-medium leading-snug">
                          {option.label}
                        </span>
                        {option.description &&
                          option.description !== option.label && (
                            <span
                              className="mt-0.5 block whitespace-normal break-all font-mono text-[0.625rem] leading-snug"
                              style={{
                                color: isSelected
                                  ? "currentColor"
                                  : "var(--text-muted)",
                              }}
                            >
                              {option.description}
                            </span>
                          )}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
            {allowNoProject && (
              <>
                <div className="my-1 h-px bg-[var(--border-subtle)]" />
                <button
                  type="button"
                  className={cn(
                    "flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-2 text-left text-xs transition-colors",
                    isStandalone
                      ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                      : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]",
                  )}
                  onClick={() => {
                    onValueChange(null);
                    setOpen(false);
                    setSearchQuery("");
                  }}
                  data-testid={`${testId ?? "project"}-standalone`}
                >
                  <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                    {isStandalone ? <Check className="h-3.5 w-3.5" /> : <CircleOff className="h-3.5 w-3.5" />}
                  </span>
                  <span>No project (standalone)</span>
                </button>
              </>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
    {isStandalone && standaloneCaption && (
      <span className="shrink-0 text-[0.6875rem] text-[var(--text-muted)]">
        {standaloneCaption}
      </span>
    )}
    </div>
  );
}
