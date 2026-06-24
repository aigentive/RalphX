import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import { useInputHistory } from "@/hooks/useInputHistory";
import {
  useAgentComposerEntries,
  useAgentComposerIntegrationResources,
  useAgentComposerPlanReferences,
  useAgentComposerSkills,
} from "@/hooks/useAgentComposerResources";
import {
  AlertCircle,
  ArrowUp,
  Bot,
  BookOpen,
  Check,
  ChevronDown,
  Cpu,
  FileText,
  FolderOpen,
  Gauge,
  GitFork,
  Loader2,
  Paperclip,
  Plus,
  Search,
  Settings,
  ScrollText,
  Square,
  Ticket,
  X,
} from "lucide-react";

import { useChatAttachmentDrop } from "@/hooks/useChatAttachmentDrop";
import type { AgentStatus } from "@/stores/chatStore";
import type { AgentProvider } from "@/stores/agentSessionStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChatAttachmentDropOverlay } from "@/components/Chat/ChatAttachmentDropOverlay";
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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { withAlpha } from "@/lib/theme-colors";
import { extractErrorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";
import {
  appendInternalSkillDirectives,
  detectAgentComposerTrigger,
  extractComposerArtifactTokens,
  extractComposerSkillTokens,
  normalizeComposerArtifactReferences,
  normalizeComposerIntegrationReferences,
  normalizeComposerProjectReferences,
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
import type { AgentComposerSkill } from "@/api/agent-composer";

interface ComposerOption {
  id: string;
  label: string;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
}

const PLAN_REFINE_COMMAND_MESSAGE =
  "Please verify and refine the current plan.";

function getSkillSourceLabel(skill: AgentComposerSkill): string {
  return skill.source === "ralphx-internal"
    ? "RalphX"
    : (skill.providerHarness ?? "native");
}

function getSlashSkillLabel(skill: AgentComposerSkill): string {
  if (skill.source === "ralphx-internal") {
    return `/${skill.name}`;
  }
  return skill.invocationValue || skill.name;
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
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerOption[];
  placeholder: string;
  disabled?: boolean;
  endAction?: ReactNode;
  testId?: string;
  className?: string;
}

interface ProviderFieldConfig {
  value: AgentProvider;
  onValueChange: (value: AgentProvider) => void;
  options: Array<ComposerOption & { id: AgentProvider }>;
  disabled?: boolean;
  footerAction?: ReactNode;
  compactFooterAction?: ReactNode;
  testId?: string;
  className?: string;
}

interface ModelFieldConfig {
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerOption[];
  disabled?: boolean;
  allowCustomValue?: boolean;
  customPlaceholder?: string | undefined;
  onOpenModelSettings?: () => void;
  testId?: string;
  className?: string;
}

interface EffortFieldConfig {
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerOption[];
  disabled?: boolean;
  testId?: string;
  className?: string;
}

interface ModeFieldConfig {
  value: string;
  onValueChange: (value: string) => void;
  onOpen?: () => void | Promise<unknown>;
  options: ComposerOption[];
  disabled?: boolean;
  testId?: string;
}

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
  projectReferences?: AgentComposerProjectReference[];
  integrationReferences?: AgentComposerIntegrationReference[];
  artifactReferences?: AgentComposerArtifactReference[];
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
   * (not focused, empty, no agent activity / attachments / references / queue).
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
  initialProjectReferences?: AgentComposerProjectReference[];
  initialIntegrationReferences?: AgentComposerIntegrationReference[];
  initialArtifactReferences?: AgentComposerArtifactReference[];
  mode?: ModeFieldConfig;
  chatFocus?: ChatFocusFieldConfig;
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
  initialProjectReferences = EMPTY_PROJECT_REFERENCES,
  initialIntegrationReferences = EMPTY_INTEGRATION_REFERENCES,
  initialArtifactReferences = EMPTY_ARTIFACT_REFERENCES,
  mode,
  chatFocus,
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
  const isControlled = controlledValue !== undefined;
  const [internalValue, setInternalValue] = useState("");
  const [isFocused, setIsFocused] = useState(false);
  const [cursorPosition, setCursorPosition] = useState(0);
  const [activeMenuIndex, setActiveMenuIndex] = useState(0);
  const [selectedInternalSkillNames, setSelectedInternalSkillNames] = useState<
    Set<string>
  >(() => new Set());
  const [selectedProjectReferences, setSelectedProjectReferences] = useState<
    Map<string, AgentComposerProjectReference>
  >(() => new Map());
  const [selectedIntegrationReferences, setSelectedIntegrationReferences] =
    useState<Map<string, AgentComposerIntegrationReference>>(() => new Map());
  const [selectedArtifactReferences, setSelectedArtifactReferences] = useState<
    Map<string, AgentComposerArtifactReference>
  >(() => new Map());
  const surfaceRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const restoreTextareaFocusOnActionMenuCloseRef = useRef(false);
  const restoreTextareaFocusCursorRef = useRef<number | null>(null);
  const hydratedInitialReferencesSignatureRef = useRef<string | null>(null);
  const value = isControlled ? controlledValue : internalValue;
  const [actionMenuOpen, setActionMenuOpen] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const isAgentAlive = agentStatus !== "idle";
  const isAgentGenerating = agentStatus === "generating";
  const canQueue = !isReadOnly && isAgentAlive;
  const shouldShowStop =
    Boolean(onStop) && isAgentGenerating && value.trim().length === 0;
  const emptySubmitValue = emptySubmitMessage?.trim() ?? "";
  const hasSubmittableValue =
    value.trim().length > 0 || emptySubmitValue.length > 0;
  const canSubmit =
    hasSubmittableValue &&
    !isReadOnly &&
    !sendDisabledReason &&
    (!isSubmitting || canQueue);
  const attachmentDisabled = isReadOnly || (isSubmitting && !canQueue);
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
    !isReadOnly && !questionMode && project.value.trim().length > 0;
  const pathQuery = activeTrigger?.kind === "path" ? activeTrigger.query : "";
  const integrationQuery =
    activeTrigger?.kind === "integration" ? activeTrigger.query : "";
  const planQuery = activeTrigger?.kind === "plan" ? activeTrigger.query : "";
  const integrationKind =
    activeTrigger?.kind === "integration"
      ? activeTrigger.integrationKind
      : null;
  const pathEntriesQuery = useAgentComposerEntries({
    projectId: project.value,
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
    projectId: project.value,
    query: planQuery,
    enabled:
      composerAssistEnabled && isFocused && activeTrigger?.kind === "plan",
  });
  const skillsQuery = useAgentComposerSkills({
    projectId: project.value,
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
      if (isControlled) {
        onChangeProp?.(nextValue);
      } else {
        setInternalValue(nextValue);
      }
      matchOptionsFromInput(nextValue);
    },
    [isControlled, matchOptionsFromInput, onChangeProp],
  );

  const { addEntry: addHistoryEntry, handleHistoryKeyDown } = useInputHistory({
    setValue,
  });

  const clearValue = useCallback(() => {
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

  const applyComposerText = useCallback(
    (nextValue: string, nextCursor: number) => {
      setValue(nextValue);
      setCursorPosition(nextCursor);
      restoreTextareaFocusCursorRef.current = nextCursor;
      focusTextareaAtComposerCursor(nextCursor);
    },
    [focusTextareaAtComposerCursor, setValue],
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
    selectedArtifactReferenceList.length > 0;

  // Collapsed (minimal) resting state. The composer expands when the textarea
  // is focused (cursor active, even with no text yet) or when there is real
  // content to keep visible: text, a live agent, attachments, references,
  // queued messages, a pending question, or read-only review. Transient
  // popovers (+, mode, model) intentionally do NOT expand it on their own, so
  // opening a menu on an unfocused composer never resizes the chat block.
  const hasComposerActivity =
    value.trim().length > 0 ||
    isAgentAlive ||
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

    textarea.style.height = "auto";
    const nextHeight = Math.min(textarea.scrollHeight, COMPOSER_MAX_HEIGHT);
    textarea.style.height = `${Math.max(nextHeight, textareaMinHeight)}px`;
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
            label: `$${skill.name}`,
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
                : "kind" in resource
                  ? `integration:${resource.kind}:${resource.id}`
                  : `integration:unknown:${resource.id}`,
          kind: "integration",
          label:
            integrationKind === "clickup"
              ? `@clickup:${"customId" in resource && resource.customId ? resource.customId : resource.id}`
              : integrationKind === "linear"
                ? `@linear:${"key" in resource ? (resource.key ?? resource.id) : resource.id}`
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
                : "kind" in resource
                  ? resource.kind
                  : "integration",
          sourceLabel:
            integrationKind === "clickup"
              ? "ClickUp"
              : integrationKind === "linear"
                ? "Linear"
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
      : "Type to search Jira, Linear, ClickUp, or Confluence");
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
        setSelectedIntegrationReferences((current) => {
          const nextSet = new Map(current);
          nextSet.set(`${reference.kind}:${reference.id}`, reference);
          return nextSet;
        });
        const next = replaceAgentComposerTrigger(value, activeTrigger, "");
        applyComposerText(next.text, next.cursor);
        return;
      }
      if (item.detail === "clear") {
        clearValue();
        return;
      }
      if (item.detail === "plan:refine") {
        if ((isSubmitting && !canQueue) || isReadOnly || sendDisabledReason) {
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
      applyComposerText,
      canQueue,
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
      if (questionMode) {
        return { message };
      }
      const tokens = new Set(extractComposerSkillTokens(message));
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
      return {
        message: withInternalSkillDirectives,
        ...(projectReferences.length > 0 ||
        normalizedIntegrationReferences.length > 0 ||
        normalizedArtifactReferences.length > 0
          ? {
              options: {
                ...(projectReferences.length > 0 ? { projectReferences } : {}),
                ...(normalizedIntegrationReferences.length > 0
                  ? { integrationReferences: normalizedIntegrationReferences }
                  : {}),
                ...(normalizedArtifactReferences.length > 0
                  ? { artifactReferences: normalizedArtifactReferences }
                  : {}),
              },
            }
          : {}),
      };
    },
    [
      questionMode,
      selectedArtifactReferenceList,
      selectedIntegrationReferenceList,
      selectedInternalSkillNames,
      selectedProjectReferenceList,
      skills,
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

  const handleSend = useCallback(async () => {
    const trimmedValue = value.trim();
    const messageValue = trimmedValue || emptySubmitValue;
    if (!messageValue) {
      if (shouldShowStop) {
        await onStop?.();
      }
      return;
    }

    if ((isSubmitting && !canQueue) || isReadOnly || sendDisabledReason) {
      return;
    }

    addHistoryEntry(messageValue);
    const outgoing = prepareMessageForSend(messageValue);

    const sendOutgoing = () =>
      outgoing.options
        ? onSend(outgoing.message, outgoing.options)
        : onSend(outgoing.message);

    if (questionMode || isControlled) {
      await sendOutgoing();
      setSelectedInternalSkillNames(new Set());
      setSelectedProjectReferences(new Map());
      setSelectedIntegrationReferences(new Map());
      setSelectedArtifactReferences(new Map());
      return;
    }

    clearValue();
    try {
      await sendOutgoing();
    } catch {
      // Errors surface through the parent; preserve the current interaction model.
    }
  }, [
    addHistoryEntry,
    canQueue,
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
          disabled={isReadOnly || (isSubmitting && !canQueue)}
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

        {(attachments.length > 0 ||
          hasSelectedReferences ||
          attachmentsUploading) && (
          <div className="px-5 pb-3">
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
                  onRemoveProjectReference={removeSelectedProjectReference}
                  onRemoveIntegrationReference={
                    removeSelectedIntegrationReference
                  }
                  onRemoveArtifactReference={removeSelectedArtifactReference}
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
              project={project}
              enableAttachments={enableAttachments}
              attachmentDisabled={attachmentDisabled}
              onOpenAttachmentPicker={handleOpenAttachmentPicker}
              {...(onForkSession
                ? {
                    onForkSession,
                    forkSessionDisabled:
                      forkSessionDisabled ||
                      isReadOnly ||
                      (isSubmitting && !canQueue),
                  }
                : {})}
              open={actionMenuOpen}
              onOpenChange={setActionMenuOpen}
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
              <ComposerRuntimePill
                provider={provider}
                model={model}
                effort={effort}
                compact={compact}
                className="max-w-[34rem]"
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
                "agent-composer-action-button ml-auto shrink-0 rounded-full text-[0.75rem] font-semibold tracking-[-0.01em] transition-[height,min-width,padding] duration-150 ease-out",
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
              ) : isSubmitting && !canQueue ? (
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
  project,
  enableAttachments,
  attachmentDisabled,
  onOpenAttachmentPicker,
  onForkSession,
  forkSessionDisabled = false,
  open,
  onOpenChange,
  onInsertIntegrationTrigger,
  onInsertPlanTrigger,
  onCloseAutoFocus,
  compact = false,
}: {
  project: ProjectFieldConfig;
  enableAttachments: boolean;
  attachmentDisabled: boolean;
  onOpenAttachmentPicker: () => void;
  onForkSession?: (() => Promise<unknown> | void) | undefined;
  forkSessionDisabled?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onInsertIntegrationTrigger: (kind: AgentComposerIntegrationKind) => void;
  onInsertPlanTrigger: () => void;
  onCloseAutoFocus?: (event: Event) => void;
  compact?: boolean;
}) {
  const hasPersistentActions = true;
  const hasPrimaryActions =
    enableAttachments || Boolean(project.endAction) || Boolean(onForkSession);
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

        {project.endAction && (
          <>
            {enableAttachments && (
              <div
                className="my-1 h-px"
                style={{ background: "var(--overlay-weak)" }}
              />
            )}
            <div className="px-1 py-1">{project.endAction}</div>
          </>
        )}

        {onForkSession && (
          <>
            {(enableAttachments || project.endAction) && (
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
        <div
          className="my-1 h-px"
          style={{ background: "var(--overlay-weak)" }}
        />
        <div className="py-1">
          <div className="px-2 py-1 text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
            Integrations
          </div>
          <div className="space-y-1">
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
              onClick={() => onInsertIntegrationTrigger("jira")}
            >
              <Search className="h-4 w-4" />
              Jira
            </button>
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
              onClick={() => onInsertIntegrationTrigger("confluence")}
            >
              <Search className="h-4 w-4" />
              Confluence
            </button>
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
              onClick={() => onInsertIntegrationTrigger("linear")}
            >
              <Search className="h-4 w-4" />
              Linear
            </button>
            <button
              type="button"
              className="flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[0.8125rem] transition-colors hover:bg-[var(--bg-hover)]"
              onClick={() => onInsertIntegrationTrigger("clickup")}
            >
              <Search className="h-4 w-4" />
              ClickUp
            </button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function ComposerReferencePills({
  projectReferences,
  integrationReferences,
  artifactReferences,
  onRemoveProjectReference,
  onRemoveIntegrationReference,
  onRemoveArtifactReference,
}: {
  projectReferences: AgentComposerProjectReference[];
  integrationReferences: AgentComposerIntegrationReference[];
  artifactReferences: AgentComposerArtifactReference[];
  onRemoveProjectReference: (path: string) => void;
  onRemoveIntegrationReference: (
    reference: AgentComposerIntegrationReference,
  ) => void;
  onRemoveArtifactReference: (
    reference: AgentComposerArtifactReference,
  ) => void;
}) {
  if (
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0
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
        const label =
          isJira || isLinear || isClickUp
            ? (reference.key ?? reference.id)
            : (reference.title ?? reference.id);
        const description =
          isJira || isLinear || isClickUp ? reference.title : reference.id;
        const typeLabel = isClickUp
          ? "ClickUp"
          : isLinear
            ? "Linear"
            : isJira
              ? "Jira"
              : "Confluence";
        return (
          <ComposerReferencePill
            key={`integration:${reference.provider}:${reference.kind}:${reference.id}`}
            testId={`agent-composer-reference-pill-integration:${reference.kind}:${reference.id}`}
            icon={isJira || isLinear || isClickUp ? Ticket : BookOpen}
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
    </div>
  );
}

function ComposerReferencePill({
  testId,
  icon: Icon,
  typeLabel,
  label,
  description,
  removeLabel,
  onRemove,
}: {
  testId: string;
  icon: ComponentType<{ className?: string }>;
  typeLabel: string;
  label: string;
  description?: string;
  removeLabel: string;
  onRemove: () => void;
}) {
  return (
    <span
      data-testid={testId}
      className="inline-flex h-9 max-w-full items-center gap-2 rounded-lg border px-2 text-[0.75rem]"
      style={{
        background: "var(--bg-surface)",
        borderColor: "var(--bg-hover)",
        color: "var(--text-primary)",
      }}
    >
      <Icon className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)]" />
      <span className="shrink-0 rounded-md border px-1.5 py-0.5 text-[0.625rem] font-medium uppercase text-[var(--text-muted)]">
        {typeLabel}
      </span>
      <span
        className="min-w-0 max-w-[16rem] truncate font-medium"
        title={label}
      >
        {label}
      </span>
      {description && description !== label ? (
        <span
          className="hidden min-w-0 max-w-[18rem] truncate text-[var(--text-muted)] sm:inline"
          title={description}
        >
          {description}
        </span>
      ) : null}
      <button
        type="button"
        className="ml-0.5 shrink-0 rounded p-0.5 text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        aria-label={removeLabel}
        onClick={onRemove}
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </span>
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
  return (
    <div className="py-1">
      <div className="px-2 py-1 text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
        Mode
      </div>
      <div className="space-y-1">
        {mode.options.map((option) => {
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
    </div>
  );
}

function ClaudeProviderIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 100 100" className={className}>
      <path
        d="m19.6 66.5 19.7-11 .3-1-.3-.5h-1l-3.3-.2-11.2-.3L14 53l-9.5-.5-2.4-.5L0 49l.2-1.5 2-1.3 2.9.2 6.3.5 9.5.6 6.9.4L38 49.1h1.6l.2-.7-.5-.4-.4-.4L29 41l-10.6-7-5.6-4.1-3-2-1.5-2-.6-4.2 2.7-3 3.7.3.9.2 3.7 2.9 8 6.1L37 36l1.5 1.2.6-.4.1-.3-.7-1.1L33 25l-6-10.4-2.7-4.3-.7-2.6c-.3-1-.4-2-.4-3l3-4.2L28 0l4.2.6L33.8 2l2.6 6 4.1 9.3L47 29.9l2 3.8 1 3.4.3 1h.7v-.5l.5-7.2 1-8.7 1-11.2.3-3.2 1.6-3.8 3-2L61 2.6l2 2.9-.3 1.8-1.1 7.7L59 27.1l-1.5 8.2h.9l1-1.1 4.1-5.4 6.9-8.6 3-3.5L77 13l2.3-1.8h4.3l3.1 4.7-1.4 4.9-4.4 5.6-3.7 4.7-5.3 7.1-3.2 5.7.3.4h.7l12-2.6 6.4-1.1 7.6-1.3 3.5 1.6.4 1.6-1.4 3.4-8.2 2-9.6 2-14.3 3.3-.2.1.2.3 6.4.6 2.8.2h6.8l12.6 1 3.3 2 1.9 2.7-.3 2-5.1 2.6-6.8-1.6-16-3.8-5.4-1.3h-.8v.4l4.6 4.5 8.3 7.5L89 80.1l.5 2.4-1.3 2-1.4-.2-9.2-7-3.6-3-8-6.8h-.5v.7l1.8 2.7 9.8 14.7.5 4.5-.7 1.4-2.6 1-2.7-.6-5.8-8-6-9-4.7-8.2-.5.4-2.9 30.2-1.3 1.5-3 1.2-2.5-2-1.4-3 1.4-6.2 1.6-8 1.3-6.4 1.2-7.9.7-2.6v-.2H49L43 72l-9 12.3-7.2 7.6-1.7.7-3-1.5.3-2.8L24 86l10-12.8 6-7.9 4-4.6-.1-.5h-.3L17.2 77.4l-4.7.6-2-2 .2-3 1-1 8-5.5Z"
        fill="currentColor"
      />
    </svg>
  );
}

function CodexProviderIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 256 260"
      preserveAspectRatio="xMidYMid"
      className={className}
    >
      <path
        d="M239.184 106.203a64.716 64.716 0 0 0-5.576-53.103C219.452 28.459 191 15.784 163.213 21.74A65.586 65.586 0 0 0 52.096 45.22a64.716 64.716 0 0 0-43.23 31.36c-14.31 24.602-11.061 55.634 8.033 76.74a64.665 64.665 0 0 0 5.525 53.102c14.174 24.65 42.644 37.324 70.446 31.36a64.72 64.72 0 0 0 48.754 21.744c28.481.025 53.714-18.361 62.414-45.481a64.767 64.767 0 0 0 43.229-31.36c14.137-24.558 10.875-55.423-8.083-76.483Zm-97.56 136.338a48.397 48.397 0 0 1-31.105-11.255l1.535-.87 51.67-29.825a8.595 8.595 0 0 0 4.247-7.367v-72.85l21.845 12.636c.218.111.37.32.409.563v60.367c-.056 26.818-21.783 48.545-48.601 48.601Zm-104.466-44.61a48.345 48.345 0 0 1-5.781-32.589l1.534.921 51.722 29.826a8.339 8.339 0 0 0 8.441 0l63.181-36.425v25.221a.87.87 0 0 1-.358.665l-52.335 30.184c-23.257 13.398-52.97 5.431-66.404-17.803ZM23.549 85.38a48.499 48.499 0 0 1 25.58-21.333v61.39a8.288 8.288 0 0 0 4.195 7.316l62.874 36.272-21.845 12.636a.819.819 0 0 1-.767 0L41.353 151.53c-23.211-13.454-31.171-43.144-17.804-66.405v.256Zm179.466 41.695-63.08-36.63L161.73 77.86a.819.819 0 0 1 .768 0l52.233 30.184a48.6 48.6 0 0 1-7.316 87.635v-61.391a8.544 8.544 0 0 0-4.4-7.213Zm21.742-32.69-1.535-.922-51.619-30.081a8.39 8.39 0 0 0-8.492 0L99.98 99.808V74.587a.716.716 0 0 1 .307-.665l52.233-30.133a48.652 48.652 0 0 1 72.236 50.391v.205ZM88.061 139.097l-21.845-12.585a.87.87 0 0 1-.41-.614V65.685a48.652 48.652 0 0 1 79.757-37.346l-1.535.87-51.67 29.825a8.595 8.595 0 0 0-4.246 7.367l-.051 72.697Zm11.868-25.58 28.138-16.217 28.188 16.218v32.434l-28.086 16.218-28.188-16.218-.052-32.434Z"
        fill="currentColor"
      />
    </svg>
  );
}

const PROVIDER_ICONS: Record<
  AgentProvider,
  ComponentType<{ className?: string }>
> = {
  claude: ClaudeProviderIcon,
  codex: CodexProviderIcon,
};

const EFFORT_BAR_COLORS: Record<string, string> = {
  low: "var(--status-error)",
  medium: "var(--accent-primary)",
  high: "var(--status-warning)",
  xhigh: "var(--status-success)",
  max: "var(--status-success)",
};

const EFFORT_ORDER = ["low", "medium", "high", "xhigh", "max"] as const;

function EffortBars({
  effortId,
  totalLevels,
  className,
}: {
  effortId: string;
  totalLevels: number;
  className?: string;
}) {
  const activeIndex = EFFORT_ORDER.indexOf(
    effortId as (typeof EFFORT_ORDER)[number],
  );
  const activeCount = activeIndex >= 0 ? activeIndex + 1 : 0;
  const color = EFFORT_BAR_COLORS[effortId] ?? "var(--text-muted)";
  return (
    <span className={cn("inline-flex items-end gap-px", className)} aria-hidden>
      {Array.from({ length: totalLevels }, (_, i) => (
        <span
          key={i}
          className="rounded-[1px]"
          style={{
            width: 3,
            height: 6 + i * 2,
            backgroundColor: i < activeCount ? color : "var(--text-muted)",
            opacity: i < activeCount ? 1 : 0.25,
          }}
        />
      ))}
    </span>
  );
}

function ComposerRuntimePill({
  provider,
  model,
  effort,
  compact = false,
  className,
}: {
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
  effort: EffortFieldConfig;
  compact?: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [viewingProvider, setViewingProvider] = useState<AgentProvider | null>(
    null,
  );
  const providerLabel =
    provider.options.find((o) => o.id === provider.value)?.label ??
    provider.value;
  const modelLabel =
    model.options.find((o) => o.id === model.value)?.label ?? model.value;
  const effortLabel =
    effort.options.find((o) => o.id === effort.value)?.label ?? effort.value;
  const runtimeSummary = [
    providerLabel,
    modelLabel,
    effortLabel ? `${effortLabel} effort` : "",
  ]
    .filter(Boolean)
    .join(" · ");

  // Hide the runtime/model pill entirely when model selection is unavailable —
  // i.e. there is no model to display and none to pick (no options or disabled).
  // This avoids an empty/disabled pill in contexts like an unresolved ideation
  // runtime or a read-only verification child chat.
  const modelText = (modelLabel ?? "").trim();
  const modelSelectionAvailable =
    modelText.length > 0 || (model.options.length > 0 && !model.disabled);
  if (!modelSelectionAvailable) {
    return null;
  }

  const hasMultipleProviders = provider.options.length > 1;
  const activeViewProvider = viewingProvider ?? provider.value;
  const viewingOption = provider.options.find(
    (o) => o.id === activeViewProvider,
  );
  const viewingProviderDisabled =
    hasMultipleProviders && (viewingOption?.disabled ?? false);
  const viewingProviderLabel = viewingOption?.label ?? activeViewProvider;

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setViewingProvider(null);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          data-testid="agent-composer-runtime-pill"
          aria-label={`Runtime: ${runtimeSummary}. Click to change.`}
          className={cn(
            "flex min-w-0 items-center gap-2 rounded-md border transition-[height,padding,background-color] duration-150 ease-out",
            compact ? "h-8 px-2.5" : "h-10 px-3",
            className,
          )}
          style={{
            background:
              "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
            borderColor: "var(--form-border)",
          }}
        >
          {(() => {
            const ActiveProviderIcon = PROVIDER_ICONS[provider.value];
            return ActiveProviderIcon ? (
              <ActiveProviderIcon className="h-3.5 w-3.5 text-[var(--text-secondary)]" />
            ) : (
              <Cpu className="h-3.5 w-3.5 text-[var(--text-secondary)]" />
            );
          })()}
          <span className="truncate text-[0.8125rem] font-medium text-[var(--text-primary)]">
            {modelLabel}
          </span>
          {effort.options.length > 0 && (
            <Tooltip delayDuration={300}>
              <TooltipTrigger asChild>
                <span className="inline-flex shrink-0">
                  <EffortBars
                    effortId={effort.value}
                    totalLevels={effort.options.length}
                  />
                </span>
              </TooltipTrigger>
              <TooltipContent side="top" className="text-xs">
                {effortLabel} effort
              </TooltipContent>
            </Tooltip>
          )}
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)]" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={6}
        onOpenAutoFocus={(e) => e.preventDefault()}
        className={cn(
          "max-h-[var(--radix-popover-content-available-height)] overflow-x-hidden overflow-y-auto overscroll-contain rounded-xl p-0",
          hasMultipleProviders ? "w-[21rem]" : "w-72",
        )}
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <div className="flex">
          {hasMultipleProviders && (
            <div
              className="flex w-12 shrink-0 flex-col gap-1 border-r p-1"
              style={{
                borderColor: "var(--border-subtle)",
                backgroundColor:
                  "color-mix(in srgb, var(--bg-base) 30%, var(--bg-elevated) 70%)",
              }}
            >
              {provider.options.map((option) => {
                const isSelected = option.id === activeViewProvider;
                const ProviderIcon = PROVIDER_ICONS[option.id] ?? Bot;
                return (
                  <div key={option.id} className="relative w-full">
                    {isSelected && (
                      <span
                        className="pointer-events-none absolute -right-1 top-1/2 z-10 h-5 w-0.5 -translate-y-1/2 rounded-l-full"
                        style={{ backgroundColor: "var(--accent-primary)" }}
                      />
                    )}
                    <Tooltip delayDuration={300}>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          data-testid={`agent-composer-runtime-provider-${option.id}`}
                          aria-label={option.label}
                          className={cn(
                            "relative flex aspect-square w-full cursor-pointer items-center justify-center rounded outline-none transition-colors",
                            isSelected
                              ? "shadow-sm"
                              : "hover:bg-[var(--bg-hover)]",
                            option.disabled && !isSelected && "opacity-50",
                          )}
                          style={
                            isSelected
                              ? {
                                  backgroundColor: "var(--bg-elevated)",
                                  color: option.disabled
                                    ? "var(--text-muted)"
                                    : "var(--text-primary)",
                                }
                              : {
                                  color: "var(--text-secondary)",
                                }
                          }
                          onClick={() => {
                            if (option.disabled) {
                              setViewingProvider(option.id);
                            } else {
                              setViewingProvider(null);
                              provider.onValueChange(option.id);
                            }
                          }}
                        >
                          <ProviderIcon className="h-5 w-5" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="right" className="text-xs">
                        {option.label}
                      </TooltipContent>
                    </Tooltip>
                  </div>
                );
              })}
              {provider.compactFooterAction && (
                <div
                  className="mt-auto border-t pt-1"
                  style={{ borderColor: "var(--border-subtle)" }}
                >
                  {provider.compactFooterAction}
                </div>
              )}
            </div>
          )}
          <div className="min-w-0 flex-1 p-1.5">
            {!hasMultipleProviders && (
              <>
                <ComposerOptionList
                  label="Provider"
                  value={provider.value}
                  options={provider.options}
                  disabled={provider.disabled ?? false}
                  testId={provider.testId ?? "agent-composer-runtime-provider"}
                  icon={Bot}
                  onValueChange={(value) => {
                    provider.onValueChange(value as AgentProvider);
                  }}
                />
                {provider.footerAction && (
                  <div className="px-1 pb-0.5 pt-1">
                    {provider.footerAction}
                  </div>
                )}
                <div
                  className="my-1 h-px"
                  style={{ background: "var(--overlay-weak)" }}
                />
              </>
            )}
            {viewingProviderDisabled ? (
              <div className="flex flex-col items-center gap-2 px-3 py-4 text-center">
                <AlertCircle
                  className="h-5 w-5"
                  style={{ color: "var(--text-muted)" }}
                />
                <span
                  className="text-[0.8125rem] font-medium"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {viewingProviderLabel} is not enabled
                </span>
                <span
                  className="text-[0.75rem] leading-snug"
                  style={{ color: "var(--text-muted)" }}
                >
                  Enable this provider in settings to use its models.
                </span>
                {provider.footerAction && (
                  <div className="mt-1 w-full">{provider.footerAction}</div>
                )}
              </div>
            ) : (
              <>
                <ComposerOptionList
                  label="Model"
                  value={model.value}
                  options={model.options}
                  disabled={model.disabled ?? false}
                  testId={model.testId ?? "agent-composer-runtime-model"}
                  icon={Cpu}
                  onValueChange={(value) => {
                    model.onValueChange(value);
                    setOpen(false);
                  }}
                />
                {model.onOpenModelSettings && (
                  <button
                    type="button"
                    className="mt-0.5 flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-[0.6875rem] transition-colors hover:bg-[var(--bg-hover)]"
                    style={{ color: "var(--text-muted)" }}
                    onClick={() => {
                      model.onOpenModelSettings?.();
                      setOpen(false);
                    }}
                  >
                    <Settings className="h-3 w-3" />
                    Manage models in Settings
                  </button>
                )}
                {effort.options.length > 0 && (
                  <>
                    <div
                      className="my-1 h-px"
                      style={{ background: "var(--overlay-weak)" }}
                    />
                    <ComposerOptionList
                      label="Effort"
                      value={effort.value}
                      options={effort.options}
                      disabled={effort.disabled ?? false}
                      testId={effort.testId ?? "agent-composer-runtime-effort"}
                      icon={Gauge}
                      onValueChange={(value) => {
                        effort.onValueChange(value);
                        setOpen(false);
                      }}
                    />
                  </>
                )}
              </>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function ComposerOptionList({
  label,
  value,
  options,
  disabled,
  testId,
  icon: Icon,
  onValueChange,
  allowCustomValue = false,
  customPlaceholder = "Custom value",
}: {
  label: string;
  value: string;
  options: ComposerOption[];
  disabled: boolean;
  testId?: string;
  icon: ComponentType<{ className?: string }>;
  onValueChange: (value: string) => void;
  allowCustomValue?: boolean;
  customPlaceholder?: string | undefined;
}) {
  const [customValue, setCustomValue] = useState("");
  const hasCurrentOption = options.some((option) => option.id === value);

  useEffect(() => {
    if (!hasCurrentOption) {
      setCustomValue(value);
    }
  }, [hasCurrentOption, value]);

  const commitCustomValue = useCallback(() => {
    const nextValue = customValue.trim();
    if (!nextValue || disabled) {
      return;
    }
    onValueChange(nextValue);
  }, [customValue, disabled, onValueChange]);

  return (
    <div className="py-1">
      <div className="flex items-center gap-1.5 px-2 py-1">
        <Icon className="h-3 w-3 text-[var(--text-muted)]" />
        <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
          {label}
        </span>
      </div>
      <div className="space-y-0.5">
        {options.map((option) => {
          const isSelected = option.id === value;
          const optionDisabled = disabled || option.disabled;
          return (
            <button
              key={option.id}
              type="button"
              disabled={optionDisabled}
              data-testid={testId ? `${testId}-${option.id}` : undefined}
              className={cn(
                "flex w-full items-start justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[0.75rem] transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                isSelected
                  ? "bg-[var(--accent-muted)]"
                  : "hover:bg-[var(--bg-hover)]",
              )}
              onClick={() => {
                if (!optionDisabled) {
                  onValueChange(option.id);
                }
              }}
            >
              <span className="min-w-0 flex-1">
                <span
                  className="block truncate"
                  style={{
                    color: isSelected
                      ? "var(--accent-primary)"
                      : "var(--text-primary)",
                    fontWeight: isSelected ? 600 : 500,
                  }}
                >
                  {option.label}
                </span>
                {(option.disabledReason || option.description) && (
                  <span
                    className="mt-0.5 block line-clamp-2 text-[0.6875rem] leading-snug"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {option.disabledReason ?? option.description}
                  </span>
                )}
              </span>
              {isSelected && (
                <Check className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent-primary)]" />
              )}
            </button>
          );
        })}
      </div>
      {allowCustomValue && (
        <div className="mt-1.5 flex items-center gap-1.5 px-1">
          <Input
            value={customValue}
            onChange={(event) => setCustomValue(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitCustomValue();
              }
            }}
            disabled={disabled}
            placeholder={customPlaceholder}
            data-testid={testId ? `${testId}-custom-input` : undefined}
            className="h-8 min-w-0 flex-1 rounded-md border-[var(--border-default)] bg-[var(--bg-surface)] px-2 text-[12px] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
          />
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={disabled || customValue.trim().length === 0}
            onClick={commitCustomValue}
            data-testid={testId ? `${testId}-custom-apply` : undefined}
            className="h-8 rounded-md px-2 text-[12px]"
          >
            Use
          </Button>
        </div>
      )}
    </div>
  );
}

export function AgentComposerProjectCreateButton({
  onClick,
  testId,
  label = "New project",
}: {
  onClick: () => void;
  testId?: string;
  label?: string;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      className="h-7 shrink-0 rounded-[10px] px-2 text-[0.625rem] font-medium"
      style={{
        color: "var(--text-secondary)",
        background: "transparent",
      }}
      onClick={onClick}
      data-testid={testId}
    >
      <Plus className="h-3.5 w-3.5" />
      {label}
    </Button>
  );
}

export function AgentComposerProjectLine({
  value,
  onValueChange,
  options,
  placeholder,
  disabled = false,
  testId,
}: ProjectFieldConfig) {
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const selectedProject = options.find((option) => option.id === value) ?? null;
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
      <FolderOpen className="h-3.5 w-3.5 shrink-0" />
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
        {selectedProject?.label ?? placeholder}
      </span>
      {!disabled && <ChevronDown className="h-3.5 w-3.5 shrink-0" />}
    </button>
  );

  if (disabled) {
    return trigger;
  }

  return (
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
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
