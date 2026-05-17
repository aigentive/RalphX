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
  ArrowUp,
  Bot,
  Check,
  ChevronDown,
  Cpu,
  FolderOpen,
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
  disabled?: boolean;
  disabledReason?: string;
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
  testId?: string;
  className?: string;
}

interface ModelFieldConfig {
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

export interface AgentComposerQuestionMode {
  optionCount: number;
  multiSelect: boolean;
  onMatchedOptions: (indices: number[]) => void;
}

export interface AgentComposerSurfaceProps {
  project: ProjectFieldConfig;
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
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
  dataTestId?: string;
  textareaTestId?: string;
  actionTestId?: string;
  submitLabel?: string;
  submittingLabel?: string;
  sendDisabledReason?: string | null;
  className?: string;
}

export function AgentComposerSurface({
  project,
  provider,
  model,
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
  dataTestId,
  textareaTestId,
  actionTestId,
  submitLabel = "Send",
  submittingLabel = "Sending...",
  sendDisabledReason = null,
  className,
}: AgentComposerSurfaceProps) {
  const isControlled = controlledValue !== undefined;
  const [internalValue, setInternalValue] = useState("");
  const [isFocused, setIsFocused] = useState(false);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const value = isControlled ? controlledValue : internalValue;
  const isAgentAlive = agentStatus !== "idle";
  const canQueue = !isReadOnly && isAgentAlive;
  const shouldShowStop = Boolean(onStop) && isAgentAlive && value.trim().length === 0;
  const canSubmit =
    value.trim().length > 0 &&
    !isReadOnly &&
    !sendDisabledReason &&
    (!isSubmitting || canQueue);
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

  const { addEntry: addHistoryEntry, handleHistoryKeyDown } = useInputHistory({
    setValue,
  });

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

    if ((isSubmitting && !canQueue) || isReadOnly || sendDisabledReason) {
      return;
    }

    addHistoryEntry(trimmedValue);

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
    addHistoryEntry,
    canQueue,
    clearValue,
    isControlled,
    isReadOnly,
    isSubmitting,
    onSend,
    onStop,
    questionMode,
    sendDisabledReason,
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
        return;
      }

      if (handleHistoryKeyDown(event, value)) {
        return;
      }
    },
    [handleHistoryKeyDown, handleSend, hasQueuedMessages, onEditLastQueued, value]
  );

  const helperText = useMemo(() => {
    if (!showHelperText) {
      return null;
    }
    return (
      <div
        className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] font-medium"
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
          borderColor: isFocused ? "var(--accent-border)" : "var(--overlay-weak)",
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
          className="block min-h-[116px] w-full resize-none border-0 bg-transparent px-5 pb-2 pt-4 text-[15px] leading-[1.5] shadow-none outline-none ring-0 focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0 sm:text-[16px]"
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
          <div className="flex flex-wrap items-center gap-2 md:flex-nowrap">
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
              {...(mode ? { mode } : {})}
            />

            <div className="flex min-w-0 flex-1 items-stretch gap-2">
              <ComposerDualSelectPill
                provider={provider}
                model={model}
                className="max-w-[340px] flex-none"
              />
            </div>

            <Button
              type="button"
              className={cn(
                "agent-composer-action-button h-10 shrink-0 rounded-[12px] px-4 text-[12px] font-semibold tracking-[-0.01em]",
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
}: {
  project: ProjectFieldConfig;
  mode?: ModeFieldConfig;
  enableAttachments: boolean;
  attachmentDisabled: boolean;
  onOpenAttachmentPicker: () => void;
}) {
  const hasPersistentActions = enableAttachments || Boolean(project.endAction) || Boolean(mode);
  const [open, setOpen] = useState(false);

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
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
            border: "1px solid var(--overlay-weak)",
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
      >
        {enableAttachments && (
          <button
            type="button"
            disabled={attachmentDisabled}
            className="flex h-10 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] transition-colors disabled:opacity-50"
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

function ComposerModeMenuSection({
  mode,
  onDone,
}: {
  mode: ModeFieldConfig;
  onDone: () => void;
}) {
  return (
    <div className="py-1">
      <div className="px-2 py-1 text-[10px] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
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
              data-testid={mode.testId ? `${mode.testId}-${option.id}` : undefined}
              className={cn(
                "flex w-full items-start gap-2 rounded-lg px-2 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                isSelected ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--bg-hover)]"
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
                {isSelected && <Check className="h-4 w-4 text-[var(--accent-primary)]" />}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium text-[var(--text-primary)]">
                  {option.label}
                </span>
                {option.description && (
                  <span className="mt-0.5 block text-[11px] leading-snug text-[var(--text-muted)]">
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

interface ComposerSelectFieldProps {
  icon: ComponentType<{ className?: string }>;
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerOption[];
  placeholder: string;
  testId?: string;
  disabled?: boolean;
  onOpenChange?: (open: boolean) => void;
  fieldClassName?: string;
}

function ComposerSelectField({
  icon: Icon,
  label,
  value,
  onValueChange,
  options,
  placeholder,
  testId,
  disabled = false,
  onOpenChange,
  fieldClassName,
}: ComposerSelectFieldProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  return (
    <div className={cn("flex min-w-0 items-center gap-2", fieldClassName)}>
      <div
        className="flex h-[24px] w-[24px] shrink-0 items-center justify-center rounded-full"
        style={{ color: "var(--text-secondary)" }}
      >
        <Icon className="h-[13px] w-[13px]" />
      </div>
      <div className="min-w-0">
        <div
          className="mb-0.5 text-[8px] font-medium uppercase tracking-[0.16em]"
          style={{ color: "var(--text-muted)" }}
        >
          {label}
        </div>
        <Select
          {...(value ? { value } : {})}
          onValueChange={onValueChange}
          disabled={disabled}
          onOpenChange={(open) => {
            onOpenChange?.(open);
            if (!open) {
              requestAnimationFrame(() => {
                triggerRef.current?.blur();
              });
            }
          }}
        >
          <SelectTrigger
            ref={triggerRef}
            className="h-auto w-auto min-w-0 border-0 bg-transparent px-0 py-0 text-[12px] font-medium shadow-none outline-none ring-0 focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0 [&>span]:max-w-full"
            style={{
              color: value ? "var(--text-primary)" : "var(--text-secondary)",
              boxShadow: "none",
              outline: "none",
              WebkitAppearance: "none",
              appearance: "none",
            }}
            data-testid={testId}
            data-theme-button-skip="true"
            aria-label={label}
          >
            <SelectValue placeholder={placeholder} />
          </SelectTrigger>
          <SelectContent>
            {options.map((option) => (
              <SelectItem key={option.id} value={option.id}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}

function ComposerDualSelectPill({
  provider,
  model,
  className,
}: {
  provider: ProviderFieldConfig;
  model: ModelFieldConfig;
  className?: string;
}) {
  const [providerOpen, setProviderOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);

  return (
    <div
      className={cn(
        "inline-flex min-h-10 max-w-full items-center gap-3 rounded-[12px] border px-2.5 py-1.5 transition-[border-color,box-shadow] focus-within:border-transparent focus-within:shadow-[0_0_0_1px_var(--accent-border)]",
        (providerOpen || modelOpen) && "border-transparent shadow-[0_0_0_1px_var(--accent-border)]",
        className
      )}
      style={{
        background: "color-mix(in srgb, var(--bg-base) 24%, var(--bg-surface) 76%)",
        borderColor: "var(--overlay-weak)",
      }}
    >
      <ComposerSelectField
        icon={Bot}
        label="Provider"
        value={provider.value}
        onValueChange={(value) => provider.onValueChange(value as AgentProvider)}
        options={provider.options}
        placeholder="Select provider"
        {...(provider.disabled !== undefined ? { disabled: provider.disabled } : {})}
        {...(provider.testId ? { testId: provider.testId } : {})}
        {...(provider.className ? { fieldClassName: provider.className } : {})}
        onOpenChange={setProviderOpen}
      />

      <div className="h-6 w-px shrink-0" style={{ background: "var(--overlay-weak)" }} />

      <ComposerSelectField
        icon={Cpu}
        label="Model"
        value={model.value}
        onValueChange={model.onValueChange}
        options={model.options}
        placeholder="Select model"
        {...(model.disabled !== undefined ? { disabled: model.disabled } : {})}
        {...(model.testId ? { testId: model.testId } : {})}
        {...(model.className ? { fieldClassName: model.className } : {})}
        onOpenChange={setModelOpen}
      />
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
      className="h-7 shrink-0 rounded-[10px] px-2 text-[10px] font-medium"
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
        "flex min-w-0 max-w-[min(100%,430px)] items-center gap-2 rounded-full px-2 py-1 text-[12px] transition-colors",
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
      <span className="shrink-0 text-[10px] font-medium uppercase tracking-[0.14em]">
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
                            className="mt-0.5 block whitespace-normal break-all font-mono text-[10px] leading-snug"
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
