import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { AgentProvider } from "@/stores/agentSessionStore";

import { ComposerRuntimeMenu } from "./ComposerRuntimeMenu";
import type { ComposerRuntimeMenuLevel } from "./ComposerRuntimeMenuRows";
import { ComposerRuntimeTrigger } from "./ComposerRuntimeTrigger";
import { runtimeSummary } from "./runtimeSelectorModel";
import type {
  ComposerRuntimeCapabilityField,
  ComposerRuntimeEffortField,
  ComposerRuntimeModelField,
  ComposerRuntimePersonaField,
  ComposerRuntimeProviderField,
  ComposerRuntimeSpeedField,
} from "./runtimeSelectorTypes";

const NARROW_RUNTIME_SELECTOR_WIDTH = 720;

interface ComposerRuntimeSelectorProps {
  provider: ComposerRuntimeProviderField;
  model: ComposerRuntimeModelField;
  effort: ComposerRuntimeEffortField;
  capability?: ComposerRuntimeCapabilityField;
  persona?: ComposerRuntimePersonaField;
  speed?: ComposerRuntimeSpeedField;
  runtimeDefault?: {
    source?: string | null;
    scopeLabel?: string;
    isResetting?: boolean;
    disabled?: boolean;
    onReset: () => Promise<unknown> | void;
  };
  runtimeTag?: string;
  compact?: boolean;
  className?: string;
  surfaceRef: RefObject<HTMLDivElement | null>;
}

export function ComposerRuntimeSelector({
  provider,
  model,
  effort,
  capability,
  persona,
  speed,
  runtimeDefault,
  runtimeTag,
  compact = false,
  className,
  surfaceRef,
}: ComposerRuntimeSelectorProps) {
  const effectiveSpeed = useMemo<ComposerRuntimeSpeedField | undefined>(() => {
    if (speed) return speed;
    const fastMode = model.fastMode;
    if (!fastMode?.visible) return undefined;
    return {
      value: fastMode.value ? "fast" : "standard",
      onValueChange: (value) => fastMode.onValueChange(value === "fast"),
      options: [
        { id: "standard", label: "Standard", description: "Default speed." },
        {
          id: "fast",
          label: "Fast",
          description: fastMode.description ?? "Use priority processing when supported.",
          ...(fastMode.disabled
            ? {
                disabled: true,
                disabledReason: fastMode.description ?? "Fast mode is unavailable.",
              }
            : {}),
        },
      ],
    };
  }, [model.fastMode, speed]);
  const [open, setOpen] = useState(false);
  const [level, setLevel] = useState<ComposerRuntimeMenuLevel>("overview");
  const [previewProvider, setPreviewProvider] = useState<AgentProvider | null>(
    null,
  );
  const [previewIndex, setPreviewIndex] = useState<number | null>(null);
  const [isNarrow, setIsNarrow] = useState(false);
  const [submenuSide, setSubmenuSide] = useState<"left" | "right">("right");
  const rootContentRef = useRef<HTMLDivElement>(null);
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
    fastMode: effectiveSpeed?.value === "fast",
  });
  const modelText = modelLabel.trim();
  const modelSelectionAvailable =
    modelText.length > 0 || (model.options.length > 0 && !model.disabled);
  const runtimeReadOnly = Boolean(
    provider.disabled && model.disabled && effort.disabled,
  );
  const showUnifiedMenu =
    !runtimeReadOnly || Boolean(capability || persona || effectiveSpeed);
  const optionSignature = useMemo(
    () => effort.options.map((option) => option.id).join("\u0000"),
    [effort.options],
  );

  useEffect(() => {
    setPreviewIndex(null);
  }, [effort.value, model.value, optionSignature]);

  useEffect(() => {
    setPreviewProvider(null);
  }, [provider.value]);

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

  useEffect(() => {
    if (!open || !showUnifiedMenu) return;
    const frame = window.requestAnimationFrame(() => {
      const bounds = rootContentRef.current?.getBoundingClientRect();
      if (!bounds || bounds.width <= 0) return;
      setSubmenuSide(
        window.innerWidth - bounds.right >= bounds.left ? "right" : "left",
      );
    });
    return () => window.cancelAnimationFrame(frame);
  }, [open, showUnifiedMenu]);

  if (!modelSelectionAvailable && !capability && !persona) return null;

  const returnToOverview = (returningFrom: ComposerRuntimeMenuLevel) => {
    setLevel("overview");
    window.requestAnimationFrame(() => {
      const triggerName = returningFrom === "models" ? "model" : returningFrom;
      rootContentRef.current
        ?.querySelector<HTMLElement>(
          `[data-testid="agent-composer-runtime-${triggerName}-menu-trigger"]`,
        )
        ?.focus();
    });
  };

  return (
    <TooltipProvider>
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (!nextOpen) {
          setLevel("overview");
          setPreviewProvider(null);
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
              runtimeSummary={summary || "Runtime settings"}
              compact={compact}
              fastMode={Boolean(
                effectiveSpeed?.value === "fast",
              )}
              {...(runtimeTag ? { scopeTag: runtimeTag } : {})}
              includesCapabilities={Boolean(capability)}
              {...(modelText.length === 0
                ? { visibleLabel: "Runtime settings" }
                : {})}
              {...(className !== undefined && { className })}
            />
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side="top" className="flex items-center gap-2">
          <span>
            Choose provider, model, effort
            {capability ? ", capabilities" : ""}
            {persona ? ", persona" : ""}
            {effectiveSpeed ? ", and speed" : ""}
          </span>
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
          if (level !== "overview") {
            event.preventDefault();
            returnToOverview(level);
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
          showUnifiedMenu
            ? "border-0 bg-transparent shadow-none"
            : "border shadow-lg",
        )}
        style={{
          backgroundColor: showUnifiedMenu
            ? "transparent"
            : "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: showUnifiedMenu ? "0" : "1px",
        }}
      >
        {showUnifiedMenu ? (
          <ComposerRuntimeMenu
            provider={provider}
            model={model}
            effort={effort}
            {...(capability ? { capability } : {})}
            {...(persona ? { persona } : {})}
            {...(effectiveSpeed ? { speed: effectiveSpeed } : {})}
            {...(runtimeDefault ? { runtimeDefault } : {})}
            viewingProvider={previewProvider ?? provider.value}
            onPreviewProviderChange={setPreviewProvider}
            level={level}
            onLevelChange={setLevel}
            narrow={isNarrow}
            side={submenuSide}
            nestedContentRef={nestedContentRef}
            previewIndex={previewIndex}
            onPreviewChange={setPreviewIndex}
          />
        ) : (
          <div className="w-[min(22rem,calc(100vw-1rem))] px-3 py-3 text-[0.75rem] text-[var(--text-secondary)]">
            {summary}
          </div>
        )}
      </PopoverContent>
    </Popover>
    </TooltipProvider>
  );
}
