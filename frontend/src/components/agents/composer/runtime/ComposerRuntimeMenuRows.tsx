import { forwardRef, type ReactNode, type RefObject } from "react";

import { ChevronRight, Loader2 } from "lucide-react";

import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export type ComposerRuntimeMenuLevel =
  | "overview"
  | "provider"
  | "models"
  | "effort"
  | "capability"
  | "persona"
  | "speed";

interface SubmenuRowProps {
  label: string;
  value: string;
  testId: string;
  expanded: boolean;
  onOpen: () => void;
  hoverToOpen: boolean;
  disabled?: boolean;
  pending?: boolean;
}

const SubmenuRow = forwardRef<HTMLButtonElement, SubmenuRowProps>(
  function SubmenuRow(
    {
      label,
      value,
      testId,
      expanded,
      onOpen,
      hoverToOpen,
      disabled = false,
      pending = false,
    },
    ref,
  ) {
    return (
      <button
        ref={ref}
        type="button"
        data-testid={testId}
        aria-label={`${label}, ${value}`}
        aria-haspopup="dialog"
        aria-expanded={expanded}
        disabled={disabled}
        className={cn(
          "flex w-full items-center gap-3 rounded-md px-2.5 py-2 text-left outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)] disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent",
          expanded && "bg-[var(--bg-hover)]",
        )}
        onPointerMove={() => {
          if (hoverToOpen && !disabled) onOpen();
        }}
        onClick={() => {
          if (!disabled) onOpen();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight" && !disabled) {
            event.preventDefault();
            onOpen();
          }
        }}
      >
        <span className="min-w-0 flex-1 truncate text-[0.75rem] font-medium text-[var(--text-primary)]">
          {label}
        </span>
        <span className="max-w-[9rem] truncate text-[0.6875rem] text-[var(--text-muted)]">
          {value}
        </span>
        {pending ? (
          <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--text-muted)]" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]" />
        )}
      </button>
    );
  },
);

function WideSubmenu({
  active,
  label,
  value,
  testId,
  side,
  triggerRef,
  contentRef,
  focusSelector,
  onOpen,
  onClose,
  disabled,
  pending,
  children,
}: {
  active: boolean;
  label: string;
  value: string;
  testId: string;
  side: "left" | "right";
  triggerRef: RefObject<HTMLButtonElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  focusSelector: string;
  onOpen: () => void;
  onClose: () => void;
  disabled?: boolean;
  pending?: boolean;
  children: ReactNode;
}) {
  const closeAndFocus = () => {
    onClose();
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  return (
    <Popover
      open={active}
      onOpenChange={(nextOpen) => (nextOpen ? onOpen() : onClose())}
    >
      <PopoverAnchor asChild>
        <SubmenuRow
          ref={triggerRef}
          label={label}
          value={value}
          testId={testId}
          expanded={active}
          onOpen={onOpen}
          hoverToOpen
          {...(disabled !== undefined && { disabled })}
          {...(pending !== undefined && { pending })}
        />
      </PopoverAnchor>
      <PopoverContent
        ref={contentRef}
        side={side}
        align="start"
        sideOffset={8}
        collisionPadding={8}
        className="max-h-[var(--radix-popover-content-available-height)] w-auto overflow-y-auto overscroll-contain border-0 bg-transparent p-0 shadow-none"
        style={{ backgroundColor: "transparent" }}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          window.requestAnimationFrame(() => {
            contentRef.current
              ?.querySelector<HTMLElement>(focusSelector)
              ?.focus();
          });
        }}
        onEscapeKeyDown={(event) => {
          event.preventDefault();
          closeAndFocus();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft") {
            event.preventDefault();
            closeAndFocus();
          }
        }}
      >
        {children}
      </PopoverContent>
    </Popover>
  );
}

export function ComposerRuntimeMenuRow({
  narrow,
  active,
  level,
  label,
  value,
  testId,
  side,
  triggerRef,
  contentRef,
  focusSelector,
  onLevelChange,
  disabled = false,
  pending = false,
  children,
}: {
  narrow: boolean;
  active: boolean;
  level: Exclude<ComposerRuntimeMenuLevel, "overview">;
  label: string;
  value: string;
  testId: string;
  side: "left" | "right";
  triggerRef: RefObject<HTMLButtonElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  focusSelector: string;
  onLevelChange: (level: ComposerRuntimeMenuLevel) => void;
  disabled?: boolean;
  pending?: boolean;
  children: ReactNode;
}) {
  if (narrow) {
    return (
      <SubmenuRow
        ref={triggerRef}
        label={label}
        value={value}
        testId={testId}
        expanded={false}
        onOpen={() => onLevelChange(level)}
        hoverToOpen={false}
        disabled={disabled}
        pending={pending}
      />
    );
  }

  return (
    <WideSubmenu
      active={active}
      label={label}
      value={value}
      testId={testId}
      side={side}
      triggerRef={triggerRef}
      contentRef={contentRef}
      focusSelector={focusSelector}
      onOpen={() => onLevelChange(level)}
      onClose={() => onLevelChange("overview")}
      disabled={disabled}
      pending={pending}
    >
      {children}
    </WideSubmenu>
  );
}
