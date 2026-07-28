import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { Check, ChevronDown } from "lucide-react";

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
import {
  LOCAL_ENVIRONMENT_ID,
  type EnvironmentConnectionState,
  type EnvironmentEntry,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

import { ENVIRONMENT_STATUS_DOT } from "./environment-switcher-status";

interface EnvironmentDotProps {
  environmentId: string;
  state: EnvironmentConnectionState;
}

function EnvironmentDot({ environmentId, state }: EnvironmentDotProps) {
  const config = ENVIRONMENT_STATUS_DOT[state];
  return (
    <span
      aria-hidden="true"
      className="inline-flex h-4 w-4 shrink-0 items-center justify-center text-[0.75rem] leading-none"
      style={{ color: config.color }}
      data-status={state}
      data-testid={`environment-dot-${environmentId}`}
    >
      {config.glyph}
    </span>
  );
}

interface EnvironmentRowProps {
  environment: EnvironmentEntry;
  state: EnvironmentConnectionState;
  selected: boolean;
  optionRef: (node: HTMLButtonElement | null) => void;
  onSelect: (id: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>, id: string) => void;
}

const EnvironmentRow = memo(function EnvironmentRow({
  environment,
  state,
  selected,
  optionRef,
  onSelect,
  onKeyDown,
}: EnvironmentRowProps) {
  const row = (
    <button
      ref={optionRef}
      type="button"
      role="option"
      aria-selected={selected}
      tabIndex={-1}
      className="flex h-9 w-full items-center gap-2 rounded-[5px] px-2 text-left text-[0.8125rem] outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
      style={{ color: "var(--text-primary)" }}
      onClick={() => onSelect(environment.id)}
      onKeyDown={(event) => onKeyDown(event, environment.id)}
      data-testid={`environment-option-${environment.id}`}
    >
      <EnvironmentDot environmentId={environment.id} state={state} />
      <span className="min-w-0 flex-1 truncate">{environment.name}</span>
      {selected ? (
        <Check
          aria-label="Active environment"
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />
      ) : null}
    </button>
  );
  const reason = environment.kind === "remote" ? ENVIRONMENT_STATUS_DOT[state].reason : null;

  if (!reason) return row;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{row}</TooltipTrigger>
      <TooltipContent side="left">{reason}</TooltipContent>
    </Tooltip>
  );
});

export interface EnvironmentSwitcherProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export const EnvironmentSwitcher = memo(function EnvironmentSwitcher({
  open,
  onOpenChange,
}: EnvironmentSwitcherProps) {
  const [internalOpen, setInternalOpen] = useState(false);
  const resolvedOpen = open ?? internalOpen;
  const setOpen = useCallback(
    (nextOpen: boolean) => {
      if (open === undefined) setInternalOpen(nextOpen);
      onOpenChange?.(nextOpen);
    },
    [onOpenChange, open],
  );
  const enabled = useUiStore((state) => state.featureFlags.remoteEnvironments);
  const environments = useEnvironmentStore((state) => state.environments);
  const activeEnvironmentId = useEnvironmentStore((state) => state.activeEnvironmentId);
  const connectionStates = useEnvironmentStore((state) => state.connectionStates);
  const setActiveEnvironment = useEnvironmentStore((state) => state.setActiveEnvironment);
  const optionRefs = useRef(new Map<string, HTMLButtonElement>());
  const triggerRef = useRef<HTMLButtonElement>(null);

  const activeEnvironment = useMemo(
    () =>
      environments.find((environment) => environment.id === activeEnvironmentId) ??
      environments[0],
    [activeEnvironmentId, environments],
  );
  const activeState =
    activeEnvironment?.kind === "local"
      ? "connected"
      : (connectionStates[activeEnvironment?.id ?? LOCAL_ENVIRONMENT_ID] ?? "idle");

  useEffect(() => {
    if (!resolvedOpen) return;
    optionRefs.current.get(activeEnvironmentId)?.focus();
  }, [activeEnvironmentId, resolvedOpen]);

  const closeAndRestoreFocus = useCallback(() => {
    setOpen(false);
    queueMicrotask(() => triggerRef.current?.focus());
  }, [setOpen]);

  const handleSelect = useCallback(
    (id: string) => {
      setOpen(false);
      if (id !== activeEnvironmentId) {
        void setActiveEnvironment(id).catch(() => undefined);
      }
      queueMicrotask(() => triggerRef.current?.focus());
    },
    [activeEnvironmentId, setActiveEnvironment, setOpen],
  );

  const handleOptionKeyDown = useCallback(
    (event: KeyboardEvent<HTMLButtonElement>, id: string) => {
      const index = environments.findIndex((environment) => environment.id === id);
      let nextIndex: number | null = null;
      switch (event.key) {
        case "ArrowDown":
          nextIndex = (index + 1) % environments.length;
          break;
        case "ArrowUp":
          nextIndex = (index - 1 + environments.length) % environments.length;
          break;
        case "Home":
          nextIndex = 0;
          break;
        case "End":
          nextIndex = environments.length - 1;
          break;
        case "Enter":
        case " ":
          event.preventDefault();
          handleSelect(id);
          return;
        case "Escape":
          event.preventDefault();
          closeAndRestoreFocus();
          return;
        default:
          return;
      }
      event.preventDefault();
      const next = environments[nextIndex];
      if (next) optionRefs.current.get(next.id)?.focus();
    },
    [closeAndRestoreFocus, environments, handleSelect],
  );

  if (!enabled || environments.length <= 1 || !activeEnvironment) return null;

  return (
    <Popover open={resolvedOpen} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <button
              ref={triggerRef}
              type="button"
              aria-label="Switch environment"
              aria-haspopup="listbox"
              aria-expanded={resolvedOpen}
              className="inline-flex h-8 max-w-[220px] items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.8125rem] font-medium outline-none transition-colors hover:bg-[var(--bg-elevated)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
              style={{
                backgroundColor: "transparent",
                borderColor: "var(--border-default)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-primary)",
              }}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                  event.preventDefault();
                  setOpen(true);
                }
              }}
              data-testid="environment-switcher-trigger"
            >
              <EnvironmentDot environmentId={activeEnvironment.id} state={activeState} />
              <span className="min-w-0 truncate">{activeEnvironment.name}</span>
              <ChevronDown
                aria-hidden="true"
                className="h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--text-muted)" }}
              />
            </button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side="bottom">Switch environment</TooltipContent>
      </Tooltip>
      <PopoverContent
        align="end"
        sideOffset={8}
        className="w-72 p-1"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          optionRefs.current.get(activeEnvironmentId)?.focus();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          triggerRef.current?.focus();
        }}
      >
        <div
          className="px-2 pb-1 pt-1.5 text-[0.625rem] font-semibold uppercase tracking-[0.12em]"
          style={{ color: "var(--text-muted)" }}
        >
          Environments
        </div>
        <div role="listbox" aria-label="Environments">
          {environments.map((environment) => {
            const state =
              environment.kind === "local"
                ? "connected"
                : (connectionStates[environment.id] ?? "idle");
            return (
              <EnvironmentRow
                key={environment.id}
                environment={environment}
                state={state}
                selected={environment.id === activeEnvironmentId}
                optionRef={(node) => {
                  if (node) optionRefs.current.set(environment.id, node);
                  else optionRefs.current.delete(environment.id);
                }}
                onSelect={handleSelect}
                onKeyDown={handleOptionKeyDown}
              />
            );
          })}
        </div>
      </PopoverContent>
    </Popover>
  );
});
