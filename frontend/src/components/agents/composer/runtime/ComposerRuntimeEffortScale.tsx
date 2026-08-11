import { useId, useRef, type KeyboardEvent, type PointerEvent } from "react";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import {
  clampOptionIndex,
  effortTone,
  optionIndexFromPointer,
  selectedOptionIndex,
} from "./runtimeSelectorModel";
import type { ComposerRuntimeOption } from "./runtimeSelectorTypes";

interface ComposerRuntimeEffortScaleProps {
  value: string;
  options: readonly ComposerRuntimeOption[];
  previewIndex: number | null;
  onPreviewChange: (index: number | null) => void;
  onCommit: (value: string) => void;
  disabled?: boolean;
  className?: string;
}

export function ComposerRuntimeEffortScale({
  value,
  options,
  previewIndex,
  onPreviewChange,
  onCommit,
  disabled = false,
  className,
}: ComposerRuntimeEffortScaleProps) {
  const descriptionId = useId();
  const pointerPreviewIndexRef = useRef<number | null>(null);
  const selectedIndex = selectedOptionIndex(options, value);
  const displayIndex = clampOptionIndex(
    previewIndex ?? selectedIndex,
    options.length,
  );
  const displayOption = options[displayIndex];
  const tone = effortTone(options, displayOption?.id ?? value);
  const percentage =
    options.length <= 1 ? 50 : (displayIndex / (options.length - 1)) * 100;
  const isInteractive = !disabled && options.length > 1;

  if (options.length === 0) return null;

  const indexForPointer = (event: PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return optionIndexFromPointer(
      event.clientX,
      bounds.left,
      bounds.width,
      options.length,
    );
  };

  const previewPointer = (event: PointerEvent<HTMLDivElement>) => {
    if (!isInteractive) return;
    const nextIndex = indexForPointer(event);
    pointerPreviewIndexRef.current = nextIndex;
    onPreviewChange(nextIndex);
  };

  const commitIndex = (index: number) => {
    const option = options[clampOptionIndex(index, options.length)];
    if (option && option.id !== value && !option.disabled) {
      onCommit(option.id);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!isInteractive) return;
    let nextIndex: number | null = null;
    if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
      nextIndex = selectedIndex - 1;
    } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
      nextIndex = selectedIndex + 1;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = options.length - 1;
    } else if (event.key === "Escape" && previewIndex !== null) {
      event.preventDefault();
      event.stopPropagation();
      onPreviewChange(null);
      return;
    }
    if (nextIndex === null) return;
    event.preventDefault();
    commitIndex(nextIndex);
  };

  return (
    <div className={cn("px-3 pb-3 pt-2", className)}>
      <div className="mb-2 flex items-center justify-between text-[0.6875rem] font-medium text-[var(--text-muted)]">
        <span>Faster</span>
        <span>Smarter</span>
      </div>
      <div
        role="slider"
        tabIndex={isInteractive ? 0 : -1}
        aria-label="Effort"
        aria-valuemin={0}
        aria-valuemax={Math.max(options.length - 1, 0)}
        aria-valuenow={displayIndex}
        aria-valuetext={displayOption?.label ?? value}
        aria-describedby={descriptionId}
        aria-disabled={!isInteractive}
        data-testid="agent-composer-runtime-effort-scale"
        className={cn(
          "relative h-7 touch-none select-none outline-none",
          isInteractive ? "cursor-pointer" : "cursor-default",
        )}
        onKeyDown={handleKeyDown}
        onPointerDown={(event) => {
          if (!isInteractive) return;
          event.preventDefault();
          event.currentTarget.setPointerCapture?.(event.pointerId);
          previewPointer(event);
        }}
        onPointerMove={(event) => {
          if (
            isInteractive &&
            event.currentTarget.hasPointerCapture?.(event.pointerId)
          ) {
            previewPointer(event);
          }
        }}
        onPointerUp={(event) => {
          if (!isInteractive) return;
          const nextIndex = pointerPreviewIndexRef.current ?? indexForPointer(event);
          event.currentTarget.releasePointerCapture?.(event.pointerId);
          pointerPreviewIndexRef.current = null;
          commitIndex(nextIndex);
          onPreviewChange(null);
        }}
        onPointerCancel={() => {
          pointerPreviewIndexRef.current = null;
          onPreviewChange(null);
        }}
      >
        <div
          className="absolute left-0 right-0 top-1/2 h-1 -translate-y-1/2 rounded-full"
          style={{ backgroundColor: "var(--overlay-moderate)" }}
        />
        <div
          className="absolute left-0 top-1/2 h-1 -translate-y-1/2 rounded-full"
          style={{ backgroundColor: tone, width: `${percentage}%` }}
        />
        {options.map((option, index) => {
          const reached = index <= displayIndex;
          const left =
            options.length <= 1 ? 50 : (index / (options.length - 1)) * 100;
          return (
            <span
              key={option.id}
              aria-hidden="true"
              className="absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border"
              style={{
                left: `${left}%`,
                backgroundColor: reached ? tone : "var(--bg-elevated)",
                borderColor: reached ? tone : "var(--border-default)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
            />
          );
        })}
        <Tooltip delayDuration={180}>
          <TooltipTrigger asChild>
            <span
              aria-hidden="true"
              className="absolute top-1/2 h-[18px] w-[18px] -translate-x-1/2 -translate-y-1/2 rounded-full border-2 shadow-sm transition-[left,box-shadow] focus-within:ring-2"
              style={{
                left: `${percentage}%`,
                backgroundColor: "var(--text-primary)",
                borderColor: "var(--bg-elevated)",
                borderStyle: "solid",
                boxShadow: `0 0 0 3px color-mix(in srgb, ${tone} 28%, transparent)`,
              }}
            />
          </TooltipTrigger>
          <TooltipContent side="top">{displayOption?.label}</TooltipContent>
        </Tooltip>
      </div>
      <div id={descriptionId} className="mt-1 min-h-12 px-0.5">
        <div className="text-[0.75rem] font-semibold text-[var(--text-primary)]">
          {displayOption?.label}
        </div>
        <div className="mt-0.5 line-clamp-2 text-[0.6875rem] leading-snug text-[var(--text-muted)]">
          {displayOption?.description ?? "\u00a0"}
        </div>
      </div>
    </div>
  );
}
