import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { ChevronRight, Zap } from "lucide-react";

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
import type { AgentProvider } from "@/stores/agentSessionStore";
import { cn } from "@/lib/utils";

import {
  ComposerRuntimeAdvancedMenu,
  type ComposerRuntimeAdvancedLevel,
} from "./ComposerRuntimeAdvancedMenu";
import { ComposerRuntimeEffortScale } from "./ComposerRuntimeEffortScale";
import { ComposerRuntimeTrigger } from "./ComposerRuntimeTrigger";
import { runtimeSummary } from "./runtimeSelectorModel";
import type {
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimeProviderField,
} from "./runtimeSelectorTypes";

const NARROW_RUNTIME_SELECTOR_WIDTH = 720;

interface ComposerRuntimeSelectorProps {
  provider: ComposerRuntimeProviderField;
  model: ComposerRuntimeModelField;
  effort: ComposerRuntimeEffortField;
  compact?: boolean;
  className?: string;
  surfaceRef: RefObject<HTMLDivElement | null>;
}

function ComposerRuntimeFastModeButton({
  model,
}: {
  model: ComposerRuntimeModelField;
}) {
  const fastMode = model.fastMode;
  if (!fastMode?.visible) return null;
  const disabled = fastMode.disabled ?? false;
  const stateLabel = fastMode.value
    ? "Fast mode is on"
    : disabled
      ? "Fast mode is unavailable"
      : "Fast mode is off";

  return (
    <Tooltip delayDuration={180}>
      <TooltipTrigger asChild>
        <span className="inline-flex shrink-0">
          <button
            type="button"
            data-testid={
              fastMode.testId ?? "agent-composer-runtime-codex-fast-mode"
            }
            aria-label={
              fastMode.value ? "Turn Fast mode off" : "Turn Fast mode on"
            }
            aria-pressed={fastMode.value}
            disabled={disabled}
            className={cn(
              "flex h-8 w-8 shrink-0 items-center justify-center rounded-md border outline-none transition-colors focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)] disabled:cursor-not-allowed disabled:opacity-45",
              fastMode.value
                ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]",
            )}
            style={{
              borderColor: fastMode.value
                ? "var(--accent-border)"
                : "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
            onClick={() => fastMode.onValueChange(!fastMode.value)}
          >
            <Zap className="h-4 w-4" aria-hidden="true" />
            <span className="sr-only">Fast mode</span>
            {fastMode.description && (
              <span className="sr-only">{fastMode.description}</span>
            )}
          </button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" className="max-w-64 leading-snug">
        <div>{stateLabel}</div>
        <div className="mt-1 font-normal opacity-80">
          {fastMode.description ?? "Use Codex priority processing when supported."}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

export function ComposerRuntimeSelector({
  provider,
  model,
  effort,
  compact = false,
  className,
  surfaceRef,
}: ComposerRuntimeSelectorProps) {
  const [open, setOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [advancedLevel, setAdvancedLevel] =
    useState<ComposerRuntimeAdvancedLevel>("overview");
  const [viewingProvider, setViewingProvider] = useState<AgentProvider>(
    provider.value,
  );
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const [isNarrow, setIsNarrow] = useState(false);
  const [advancedSide, setAdvancedSide] = useState<"left" | "right">("right");
  const advancedTriggerRef = useRef<HTMLButtonElement>(null);
  const rootContentRef = useRef<HTMLDivElement>(null);
  const quickContentRef = useRef<HTMLDivElement>(null);
  const nestedContentRef = useRef<HTMLDivElement>(null);

  const providerLabel =
    provider.options.find((option) => option.id === provider.value)?.label ??
    provider.value;
  const modelLabel =
    model.options.find((option) => option.id === model.value)?.label ??
    model.value;
  const effortLabel =
    effort.options.find((option) => option.id === effort.value)?.label ??
    effort.value;
  const summary = runtimeSummary({
    providerLabel,
    modelLabel,
    effortLabel,
    fastMode: Boolean(model.fastMode?.visible && model.fastMode.value),
  });
  const modelText = modelLabel.trim();
  const modelSelectionAvailable =
    modelText.length > 0 || (model.options.length > 0 && !model.disabled);
  const runtimeReadOnly = Boolean(
    provider.disabled && model.disabled && effort.disabled,
  );

  const optionSignature = useMemo(
    () => effort.options.map((option) => option.id).join("\u0000"),
    [effort.options],
  );

  useEffect(() => {
    setPreviewIndex(null);
  }, [effort.value, model.value, optionSignature]);

  useEffect(() => {
    if (!open) setViewingProvider(provider.value);
  }, [open, provider.value]);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const updateWidth = () => {
      const width = surface.getBoundingClientRect().width;
      setIsNarrow(width > 0 && width <= NARROW_RUNTIME_SELECTOR_WIDTH);
    };
    updateWidth();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateWidth);
    observer.observe(surface);
    return () => observer.disconnect();
  }, [surfaceRef]);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const openFromShortcut = (event: KeyboardEvent) => {
      if (
        event.ctrlKey &&
        event.shiftKey &&
        event.key.toLowerCase() === "m" &&
        surface.contains(event.target as Node)
      ) {
        event.preventDefault();
        setOpen(true);
      }
    };
    surface.addEventListener("keydown", openFromShortcut);
    return () => surface.removeEventListener("keydown", openFromShortcut);
  }, [surfaceRef]);

  if (!modelSelectionAvailable) return null;

  const closeAdvanced = () => {
    setAdvancedOpen(false);
    setAdvancedLevel("overview");
    setPreviewIndex(null);
    window.requestAnimationFrame(() => advancedTriggerRef.current?.focus());
  };

  const setAdvancedOpenWithPlacement = (nextOpen: boolean) => {
    if (nextOpen && rootContentRef.current) {
      const bounds = rootContentRef.current.getBoundingClientRect();
      if (bounds.width > 0) {
        setAdvancedSide(
          window.innerWidth - bounds.right >= bounds.left ? "right" : "left",
        );
      }
    }
    setAdvancedOpen(nextOpen);
    if (!nextOpen) setAdvancedLevel("overview");
  };

  const advancedMenu = (
    <ComposerRuntimeAdvancedMenu
      provider={provider}
      model={model}
      effort={effort}
      viewingProvider={viewingProvider}
      onViewingProviderChange={setViewingProvider}
      level={advancedLevel}
      onLevelChange={setAdvancedLevel}
      narrow={isNarrow}
      side={advancedSide}
      nestedContentRef={nestedContentRef}
      onBackToQuick={closeAdvanced}
    />
  );

  return (
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (!nextOpen) {
          setAdvancedOpen(false);
          setAdvancedLevel("overview");
          setViewingProvider(provider.value);
          setPreviewIndex(null);
        }
      }}
    >
      <Tooltip delayDuration={240}>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <ComposerRuntimeTrigger
              provider={provider.value}
              modelLabel={modelLabel}
              effortValue={effort.value}
              effortLabel={effortLabel}
              effortOptions={effort.options}
              runtimeSummary={summary}
              compact={compact}
              fastMode={Boolean(
                model.fastMode?.visible && model.fastMode.value,
              )}
              {...(className !== undefined && { className })}
            />
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side="top" className="flex items-center gap-2">
          <span>Choose provider, model, and effort</span>
          <kbd className="rounded border border-[var(--tooltip-border)] px-1.5 py-0.5 text-[0.625rem] font-semibold">
            ⌃⇧M
          </kbd>
        </TooltipContent>
      </Tooltip>

      <PopoverContent
        ref={rootContentRef}
        side="top"
        align="end"
        sideOffset={6}
        collisionPadding={8}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => {
          if (advancedOpen) {
            event.preventDefault();
            if (isNarrow && advancedLevel !== "overview") {
              const returningFrom = advancedLevel;
              setAdvancedLevel("overview");
              window.requestAnimationFrame(() => {
                const triggerName =
                  returningFrom === "models" ? "model" : returningFrom;
                rootContentRef.current
                  ?.querySelector<HTMLElement>(
                    `[data-testid="agent-composer-runtime-${triggerName}-menu-trigger"]`,
                  )
                  ?.focus();
              });
            } else {
              closeAdvanced();
            }
          }
        }}
        onInteractOutside={(event) => {
          const target = event.target as Node;
          if (nestedContentRef.current?.contains(target)) {
            event.preventDefault();
          }
        }}
        className={cn(
          "w-auto max-w-[calc(100vw-1rem)] overflow-visible rounded-xl p-0",
          advancedOpen ? "border-0 bg-transparent shadow-none" : "border",
        )}
        style={{
          backgroundColor: advancedOpen
            ? "transparent"
            : "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: advancedOpen ? "0" : "1px",
        }}
      >
        {advancedOpen ? (
          advancedMenu
        ) : (
          <div
            ref={quickContentRef}
            data-testid="agent-composer-runtime-quick"
            className="w-[min(22rem,calc(100vw-1rem))]"
          >
            {runtimeReadOnly ? (
              <div className="px-3 py-3 text-[0.75rem] text-[var(--text-secondary)]">
                {summary}
              </div>
            ) : (
              <>
                <div className="flex items-center gap-2 p-2 pb-1">
                  <button
                    ref={advancedTriggerRef}
                    type="button"
                    aria-label="Advanced provider and model settings"
                    aria-expanded={advancedOpen}
                    className="flex h-8 min-w-0 flex-1 items-center justify-between rounded-md px-2 text-[0.75rem] font-medium text-[var(--text-secondary)] outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)]"
                    onClick={() => setAdvancedOpenWithPlacement(true)}
                    onKeyDown={(event) => {
                      if (event.key === "ArrowRight") {
                        event.preventDefault();
                        setAdvancedOpenWithPlacement(true);
                      }
                    }}
                  >
                    <span>Advanced</span>
                    <ChevronRight className="h-3.5 w-3.5" />
                  </button>
                  <ComposerRuntimeFastModeButton model={model} />
                </div>
                {effort.options.length > 0 && (
                  <ComposerRuntimeEffortScale
                    value={effort.value}
                    options={effort.options}
                    previewIndex={previewIndex}
                    onPreviewChange={setPreviewIndex}
                    onCommit={effort.onValueChange}
                    disabled={effort.disabled ?? false}
                  />
                )}
              </>
            )}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
