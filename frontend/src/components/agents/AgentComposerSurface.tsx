import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import {
  ArrowUp,
  Bot,
  Check,
  ChevronDown,
  Cpu,
  FolderOpen,
  Gauge,
  Loader2,
  Paperclip,
  Plus,
  Search,
  Square,
} from "lucide-react";

import type { AgentStatus } from "@/stores/chatStore";
import type { AgentProvider } from "@/stores/agentSessionStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  ChatAttachmentGallery,
  type ChatAttachment as ComposerAttachment,
} from "@/components/Chat/ChatAttachmentGallery";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { withAlpha } from "@/lib/theme-colors";
import { cn } from "@/lib/utils";

const COMPOSER_ATTACHMENT_ACCEPTED_TYPES = [
  "text/*",
  "image/*",
  "application/pdf",
  "application/json",
  ".md",
  ".txt",
  ".js",
  ".ts",
  ".tsx",
  ".jsx",
  ".py",
  ".rs",
  ".go",
  ".java",
  ".cpp",
  ".c",
  ".h",
].join(",");

const COMPOSER_ATTACHMENT_MAX_FILES = 5;
const COMPOSER_ATTACHMENT_MAX_FILE_SIZE = 10 * 1024 * 1024;

interface ComposerOption {
  id: string;
  label: string;
  description?: string;
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
  options: Array<{ id: AgentProvider; label: string }>;
  disabled?: boolean;
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

export interface AgentComposerSurfaceProps {
  project: ProjectFieldConfig;
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
  effort: EffortFieldConfig;
  onSend: (message: string) => Promise<void> | void;
  onStop?: (() => Promise<unknown> | void) | undefined;
  placeholder?: string;
  isSubmitting?: boolean;
  agentStatus?: AgentStatus;
  value?: string;
  onChange?: (value: string) => void;
  isReadOnly?: boolean;
  autoFocus?: boolean;
  showHelperText?: boolean;
  questionMode?: AgentComposerQuestionMode;
  hasQueuedMessages?: boolean;
  onEditLastQueued?: (() => void) | undefined;
  attachments?: ComposerAttachment[];
  enableAttachments?: boolean;
  onFilesSelected?: ((files: File[]) => void | Promise<unknown>) | undefined;
  onRemoveAttachment?: ((id: string) => void | Promise<unknown>) | undefined;
  attachmentsUploading?: boolean;
  mode?: ModeFieldConfig;
  chatFocus?: ChatFocusFieldConfig;
  dataTestId?: string;
  textareaTestId?: string;
  actionTestId?: string;
  submitLabel?: string;
  submittingLabel?: string;
  className?: string;
}

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
  isReadOnly = false,
  autoFocus = false,
  showHelperText = true,
  questionMode,
  hasQueuedMessages = false,
  onEditLastQueued,
  attachments = [],
  enableAttachments = false,
  onFilesSelected,
  onRemoveAttachment,
  attachmentsUploading = false,
  mode,
  chatFocus,
  dataTestId,
  textareaTestId,
  actionTestId,
  submitLabel = "Send",
  submittingLabel = "Sending...",
  className,
}: AgentComposerSurfaceProps) {
  const isControlled = controlledValue !== undefined;
  const [internalValue, setInternalValue] = useState("");
  const [isFocused, setIsFocused] = useState(false);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const value = isControlled ? controlledValue : internalValue;
  const [actionMenuOpen, setActionMenuOpen] = useState(false);
  const isAgentAlive = agentStatus !== "idle";
  const canQueue = !isReadOnly && isAgentAlive;
  const shouldShowStop = Boolean(onStop) && isAgentAlive && value.trim().length === 0;
  const canSubmit = value.trim().length > 0 && !isReadOnly && (!isSubmitting || canQueue);
  const attachmentDisabled = isReadOnly || (isSubmitting && !canQueue);
  const effectivePlaceholder = isReadOnly
    ? "Viewing historical state (read-only)"
    : questionMode
      ? `Type 1-${questionMode.optionCount} or a custom response...`
      : placeholder;

  useEffect(() => {
    if (autoFocus && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [autoFocus]);

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }

    textarea.style.height = "auto";
    const nextHeight = Math.min(textarea.scrollHeight, 220);
    textarea.style.height = `${Math.max(nextHeight, 116)}px`;
  }, [value]);

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
    [questionMode]
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
    [isControlled, matchOptionsFromInput, onChangeProp]
  );

  const clearValue = useCallback(() => {
    if (isControlled) {
      onChangeProp?.("");
    } else {
      setInternalValue("");
    }
    questionMode?.onMatchedOptions([]);
  }, [isControlled, onChangeProp, questionMode]);

  const handleAttachmentSelect = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const fileList = event.target.files;
      if (!fileList || fileList.length === 0) {
        return;
      }

      const validFiles = Array.from(fileList)
        .filter((file) => file.size <= COMPOSER_ATTACHMENT_MAX_FILE_SIZE)
        .slice(0, COMPOSER_ATTACHMENT_MAX_FILES);

      if (validFiles.length > 0) {
        void onFilesSelected?.(validFiles);
      }

      event.target.value = "";
    },
    [onFilesSelected]
  );

  const handleOpenAttachmentPicker = useCallback(() => {
    if (!attachmentDisabled) {
      fileInputRef.current?.click();
    }
  }, [attachmentDisabled]);

  const handleSend = useCallback(async () => {
    const trimmedValue = value.trim();
    if (!trimmedValue) {
      if (shouldShowStop) {
        await onStop?.();
      }
      return;
    }

    if ((isSubmitting && !canQueue) || isReadOnly) {
      return;
    }

    if (questionMode || isControlled) {
      await onSend(trimmedValue);
      return;
    }

    clearValue();
    try {
      await onSend(trimmedValue);
    } catch {
      // Errors surface through the parent; preserve the current interaction model.
    }
  }, [
    canQueue,
    clearValue,
    isControlled,
    isReadOnly,
    isSubmitting,
    onSend,
    onStop,
    questionMode,
    shouldShowStop,
    value,
  ]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
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
      }
    },
    [handleSend, hasQueuedMessages, onEditLastQueued, value]
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
        <span>{shouldShowStop ? "Stop the active run" : "Press Enter to send"}</span>
        <span aria-hidden="true" style={{ color: "var(--overlay-moderate)" }}>
          •
        </span>
        <span>Shift + Enter for a new line</span>
      </div>
    );
  }, [shouldShowStop, showHelperText]);

  return (
    <div
      ref={surfaceRef}
      data-testid={dataTestId}
      className={cn("agent-composer-surface mx-auto w-full max-w-full", className)}
    >
      <div
        className="overflow-hidden rounded-[22px] border transition-colors"
        style={{
          background: "var(--bg-surface)",
          borderColor: isFocused ? "var(--accent-border)" : "var(--form-border)",
          boxShadow: "var(--shadow-sm)",
        }}
      >
        <textarea
          ref={textareaRef}
          data-testid={textareaTestId}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={handleKeyDown}
          onFocus={() => setIsFocused(true)}
          onBlur={() => setIsFocused(false)}
          disabled={isReadOnly || (isSubmitting && !canQueue)}
          placeholder={effectivePlaceholder}
          className="block min-h-[116px] w-full resize-none border-0 bg-transparent px-5 pb-2 pt-4 text-[0.9375rem] leading-[1.5] shadow-none outline-none ring-0 focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0 sm:text-[1rem]"
          style={{
            color: "var(--text-primary)",
            boxShadow: "none",
            outline: "none",
            WebkitAppearance: "none",
            appearance: "none",
          }}
          aria-label="Message input"
        />

        {(attachments.length > 0 || attachmentsUploading || helperText) && (
          <div className="px-5 pb-3">
            {attachments.length > 0 && (
              <div className="pb-3">
                <ChatAttachmentGallery
                  attachments={attachments}
                  {...(onRemoveAttachment ? { onRemove: onRemoveAttachment } : {})}
                  uploading={attachmentsUploading}
                  compact
                />
              </div>
            )}
            {helperText}
          </div>
        )}

        <div
          className="border-t px-3.5 py-2"
          style={{
            borderColor: "var(--overlay-faint)",
            background: "color-mix(in srgb, var(--bg-base) 16%, var(--bg-surface) 84%)",
          }}
        >
          <div className="flex flex-wrap items-center gap-2">
            {enableAttachments && (
              <input
                ref={fileInputRef}
                data-testid="attachment-file-input"
                type="file"
                multiple
                accept={COMPOSER_ATTACHMENT_ACCEPTED_TYPES}
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
              open={actionMenuOpen}
              onOpenChange={setActionMenuOpen}
              {...(mode ? { mode } : {})}
            />

            {mode && (
              <ComposerModeChip
                mode={mode}
                onClick={() => setActionMenuOpen((prev) => !prev)}
              />
            )}

            {chatFocus && chatFocus.options.length > 1 && (
              <ComposerChatFocusPill chatFocus={chatFocus} />
            )}

            <div className="flex min-w-0 flex-1 items-stretch gap-2">
              <ComposerRuntimePill
                provider={provider}
                model={model}
                effort={effort}
                className="flex-none"
              />
            </div>

            <Button
              type="button"
              className={cn(
                "agent-composer-action-button h-10 shrink-0 rounded-[12px] px-4 text-[0.75rem] font-semibold tracking-[-0.01em]",
                shouldShowStop ? "min-w-[100px]" : "min-w-[118px]"
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
                  <span className="agent-composer-action-label">{submittingLabel}</span>
                </>
              ) : (
                <>
                  <ArrowUp className="h-4 w-4" />
                  <span className="agent-composer-action-label">{submitLabel}</span>
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ComposerActionMenu({
  project,
  mode,
  enableAttachments,
  attachmentDisabled,
  onOpenAttachmentPicker,
  open,
  onOpenChange,
}: {
  project: ProjectFieldConfig;
  mode?: ModeFieldConfig;
  enableAttachments: boolean;
  attachmentDisabled: boolean;
  onOpenAttachmentPicker: () => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const hasPersistentActions = enableAttachments || Boolean(project.endAction) || Boolean(mode);
  const setOpen = onOpenChange;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            "agent-composer-plus-trigger flex h-10 w-10 shrink-0 items-center justify-center rounded-[12px] transition-colors disabled:opacity-40",
            !hasPersistentActions && "agent-composer-compact-only"
          )}
          style={{
            background: "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
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
        onInteractOutside={(event) => {
          const target = event.target as HTMLElement | null;
          if (target?.closest("[data-composer-mode-chip='true']")) {
            event.preventDefault();
          }
        }}
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
              <div className="my-1 h-px" style={{ background: "var(--overlay-weak)" }} />
            )}
            <div className="px-1 py-1">{project.endAction}</div>
          </>
        )}

        {mode && (
          <>
            {(enableAttachments || project.endAction) && (
              <div className="my-1 h-px" style={{ background: "var(--overlay-weak)" }} />
            )}
            <ComposerModeMenuSection mode={mode} onDone={() => setOpen(false)} />
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

function ComposerModeChip({
  mode,
  onClick,
}: {
  mode: ModeFieldConfig;
  onClick?: () => void;
}) {
  const activeOption = mode.options.find((o) => o.id === mode.value);
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={mode.disabled}
      data-testid={mode.testId ? `${mode.testId}-chip` : "agent-composer-mode-chip"}
      data-composer-mode-chip="true"
      aria-label={`Mode: ${activeOption?.label ?? mode.value}. Click to change.`}
      className="inline-flex h-10 shrink-0 items-center gap-2 rounded-[12px] border px-3 transition-colors hover:bg-[var(--bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
      style={{
        background: "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
        borderColor: "var(--form-border)",
      }}
    >
      <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
        Mode
      </span>
      <span className="text-[0.8125rem] font-medium text-[var(--text-primary)]">
        {activeOption?.label ?? "—"}
      </span>
    </button>
  );
}

function ComposerChatFocusPill({ chatFocus }: { chatFocus: ChatFocusFieldConfig }) {
  const [open, setOpen] = useState(false);
  const activeOption =
    chatFocus.options.find((o) => o.id === chatFocus.value) ?? chatFocus.options[0];
  const ActiveIcon = activeOption?.icon;
  const triggerStyle = activeOption?.toneColor
    ? {
        background: activeOption.toneBackground ?? "var(--bg-surface)",
        borderColor: activeOption.toneBorder ?? "var(--form-border)",
        color: activeOption.toneColor,
      }
    : {
        background: "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
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
            chatFocus.testId ? `${chatFocus.testId}-pill` : "agent-composer-chat-focus-pill"
          }
          aria-label={`Chat focus: ${activeOption?.label ?? chatFocus.value}. Click to change.`}
          className="flex h-10 shrink-0 items-center gap-2 rounded-[12px] border px-3 transition-colors disabled:opacity-50"
          style={triggerStyle}
        >
          <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
            Chat
          </span>
          <span className="flex items-center gap-1.5 text-[0.8125rem] font-medium">
            {ActiveIcon ? <ActiveIcon className="h-3.5 w-3.5" /> : null}
            <span>{activeOption?.label ?? "—"}</span>
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
          const optionStyle = selected && option.toneColor
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
          return (
            <button
              key={option.id}
              type="button"
              disabled={mode.disabled}
              data-testid={mode.testId ? `${mode.testId}-${option.id}` : undefined}
              className={cn(
                "flex w-full items-start gap-2 rounded-lg px-2 py-2 text-left transition-colors disabled:opacity-50",
                isSelected ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--bg-hover)]"
              )}
              onClick={() => {
                mode.onValueChange(option.id);
                onDone();
              }}
            >
              <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
                {isSelected && <Check className="h-4 w-4 text-[var(--accent-primary)]" />}
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
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ComposerRuntimePill({
  provider,
  model,
  effort,
  className,
}: {
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
  effort: EffortFieldConfig;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const providerLabel =
    provider.options.find((o) => o.id === provider.value)?.label ?? provider.value;
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

  // Skip the pill entirely when we have nothing to show (e.g., child chat
  // session whose effective runtime hasn't loaded yet). Avoids an empty
  // "·" placeholder.
  if (!providerLabel && !modelLabel && !effortLabel) {
    return null;
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-testid="agent-composer-runtime-pill"
          aria-label={`Runtime: ${runtimeSummary}. Click to change.`}
          className={cn(
            "flex h-10 shrink-0 items-center gap-2 rounded-[12px] border px-3 transition-colors",
            className
          )}
          style={{
            background: "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
            borderColor: "var(--form-border)",
          }}
        >
          <Cpu className="h-3.5 w-3.5 text-[var(--text-secondary)]" />
          <span className="truncate text-[0.8125rem] font-medium text-[var(--text-primary)]">
            <span className="text-[var(--text-secondary)]">{providerLabel}</span>
            <span className="px-1 text-[var(--text-muted)]">·</span>
            <span>{modelLabel}</span>
            {effortLabel && (
              <>
                <span className="px-1 text-[var(--text-muted)]">·</span>
                <span>{effortLabel}</span>
              </>
            )}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)]" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={6}
        className="w-72 rounded-xl p-1.5"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <ComposerOptionList
          label="Provider"
          value={provider.value}
          options={provider.options.map((o) => ({ id: o.id, label: o.label }))}
          disabled={provider.disabled ?? false}
          testId={provider.testId ?? "agent-composer-runtime-provider"}
          icon={Bot}
          onValueChange={(value) => {
            provider.onValueChange(value as AgentProvider);
          }}
        />
        <div className="my-1 h-px" style={{ background: "var(--overlay-weak)" }} />
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
          allowCustomValue={model.allowCustomValue ?? false}
          customPlaceholder={model.customPlaceholder}
        />
        {effort.options.length > 0 && (
          <>
            <div className="my-1 h-px" style={{ background: "var(--overlay-weak)" }} />
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
          return (
            <button
              key={option.id}
              type="button"
              disabled={disabled}
              data-testid={testId ? `${testId}-${option.id}` : undefined}
              className={cn(
                "flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-[0.75rem] transition-colors disabled:opacity-50",
                isSelected ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--bg-hover)]"
              )}
              onClick={() => onValueChange(option.id)}
            >
              <span
                className="truncate"
                style={{
                  color: isSelected
                    ? "var(--accent-primary)"
                    : "var(--text-primary)",
                  fontWeight: isSelected ? 600 : 500,
                }}
              >
                {option.label}
              </span>
              {isSelected && (
                <Check className="h-3.5 w-3.5 shrink-0 text-[var(--accent-primary)]" />
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
        option.description?.toLowerCase().includes(query)
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
        "disabled:cursor-not-allowed disabled:opacity-60"
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
        style={{ color: selectedProject ? "var(--text-primary)" : "var(--text-secondary)" }}
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
                          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
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
                        {option.description && option.description !== option.label && (
                          <span
                            className="mt-0.5 block whitespace-normal break-all font-mono text-[0.625rem] leading-snug"
                            style={{ color: isSelected ? "currentColor" : "var(--text-muted)" }}
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
