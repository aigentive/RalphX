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
import { toast } from "sonner";

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
import {
  isRemoteTransportError,
  parseRemoteTransportErrorCode,
} from "@/lib/remote/transport-errors";

import { RemoteConnectionJournalDialog } from "@/components/remote/RemoteConnectionJournalDialog";

import {
  environmentDotConfig,
  environmentStatusReason,
} from "./environment-switcher-status";
import type { AttemptFailure } from "@/lib/remote/supervisor";
import type { SupervisorPresentation } from "@/lib/remote/supervisor-transition-table";

interface EnvironmentDotProps {
  environmentId: string;
  state: EnvironmentConnectionState;
  presentation?: SupervisorPresentation | undefined;
  isActive?: boolean;
}

function EnvironmentDot({
  environmentId,
  state,
  presentation,
  isActive = false,
}: EnvironmentDotProps) {
  const config = environmentDotConfig(state, presentation, isActive);
  return (
    <span
      aria-hidden="true"
      className={`inline-flex h-4 w-4 shrink-0 items-center justify-center text-[0.75rem] leading-none${config.pulseClass ? ` ${config.pulseClass}` : ""}`}
      style={{ color: config.color }}
      data-status={config.pulseClass ? "syncing" : state}
      data-testid={`environment-dot-${environmentId}`}
    >
      {config.glyph}
    </span>
  );
}

/** Counts above this render as `9+`; the exact number stays in the accessible name. */
const BADGE_DISPLAY_CAP = 9;

/** The spoken count, so a screen reader never has to interpret the `9+` glyph. */
function badgeLabel(count: number): string {
  return count === 1
    ? "1 new notification"
    : `${String(count)} new notifications`;
}

/**
 * The observed-notification tally for a BACKGROUND environment (PR 3.3-a).
 *
 * Renders nothing at zero rather than an empty chip: a badge slot that is always
 * present reads as "0 unread" to a screen reader and adds visual noise to the common
 * case. Paint uses literal colors and longhand properties per rule 22 — the switcher
 * lives in the top bar, where a dropped `var()` chain would render an invisible chip on
 * WKWebView while Chromium looked correct.
 */
function NotificationBadge({
  count,
  testId,
}: {
  count: number;
  testId: string;
}): React.ReactElement | null {
  if (count <= 0) return null;
  return (
    <span
      className="grid h-4 min-w-4 shrink-0 place-items-center rounded-full px-1 text-[0.625rem] font-bold leading-none"
      style={{
        backgroundColor: "var(--accent-primary)",
        color: "var(--text-on-accent, #ffffff)",
      }}
      data-testid={testId}
      aria-label={badgeLabel(count)}
    >
      {count > BADGE_DISPLAY_CAP ? `${String(BADGE_DISPLAY_CAP)}+` : count}
    </span>
  );
}

interface EnvironmentRowProps {
  environment: EnvironmentEntry;
  state: EnvironmentConnectionState;
  presentation: SupervisorPresentation | undefined;
  blockedFailure: AttemptFailure | null;
  badgeCount: number;
  selected: boolean;
  optionRef: (node: HTMLButtonElement | null) => void;
  onSelect: (id: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>, id: string) => void;
}

const EnvironmentRow = memo(function EnvironmentRow({
  environment,
  state,
  presentation,
  blockedFailure,
  badgeCount,
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
      <EnvironmentDot
        environmentId={environment.id}
        state={state}
        presentation={presentation}
        isActive={selected}
      />
      <span className="min-w-0 flex-1 truncate">{environment.name}</span>
      <NotificationBadge
        count={badgeCount}
        testId={`environment-badge-${environment.id}`}
      />
      {selected ? (
        <Check
          aria-label="Active environment"
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />
      ) : null}
    </button>
  );
  const reason =
    environment.kind === "remote"
      ? environmentStatusReason(state, blockedFailure, presentation, selected)
      : null;

  if (!reason) return row;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{row}</TooltipTrigger>
      <TooltipContent side="left">{reason}</TooltipContent>
    </Tooltip>
  );
});

/** The transport code where there is one, so the toast names the actual refusal. */
function switchFailureDetail(error: unknown): string {
  if (isRemoteTransportError(error)) {
    return error.code;
  }
  const code = parseRemoteTransportErrorCode(error);
  if (code !== null) {
    return code;
  }
  return error instanceof Error ? error.message : String(error);
}

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
  const [journalOpen, setJournalOpen] = useState(false);
  const environments = useEnvironmentStore((state) => state.environments);
  const activeEnvironmentId = useEnvironmentStore((state) => state.activeEnvironmentId);
  const connectionStates = useEnvironmentStore((state) => state.connectionStates);
  const connectionPresentations = useEnvironmentStore(
    (state) => state.connectionPresentations,
  );
  const notificationBadges = useEnvironmentStore((state) => state.notificationBadges);
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
  const activePresentation =
    activeEnvironment?.kind === "remote"
      ? connectionPresentations[activeEnvironment.id]?.presentation
      : undefined;
  const activeSyncing = activePresentation === "syncing";

  /**
   * The collapsed trigger cannot show a per-environment badge, so it carries the SUM
   * across background environments — the one number that answers "is there anything
   * waiting behind this menu". The active environment is excluded because it is
   * projecting: its notifications are already in its own cache and its own bell.
   */
  const backgroundBadgeTotal = useMemo(
    () =>
      Object.entries(notificationBadges).reduce(
        (total, [id, count]) => (id === activeEnvironmentId ? total : total + count),
        0,
      ),
    [activeEnvironmentId, notificationBadges],
  );

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
        const name =
          environments.find((environment) => environment.id === id)?.name ?? id;
        void setActiveEnvironment(id).catch((error: unknown) => {
          // Rust refused the switch and the store already reverted. Swallowing the
          // rejection would turn a backend refusal into a UI no-op: a double remount
          // flicker, in-flight REMOTE_FORBIDDEN failures, then a silent revert.
          toast.error(`Could not switch to ${name}`, {
            description: switchFailureDetail(error),
          });
        });
      }
      queueMicrotask(() => triggerRef.current?.focus());
    },
    [activeEnvironmentId, environments, setActiveEnvironment, setOpen],
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
              aria-label={[
                activeSyncing
                  ? `Switch environment, syncing with "${activeEnvironment.name}"`
                  : "Switch environment",
                ...(backgroundBadgeTotal > 0
                  ? [`${badgeLabel(backgroundBadgeTotal)} in other environments`]
                  : []),
              ].join(", ")}
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
              <EnvironmentDot
                environmentId={activeEnvironment.id}
                state={activeState}
                presentation={activePresentation}
                isActive
              />
              <span className="min-w-0 truncate">{activeEnvironment.name}</span>
              {activeSyncing ? (
                <span
                  className="shrink-0"
                  style={{ color: "var(--text-muted)" }}
                  data-testid="environment-switcher-syncing-label"
                >
                  Syncing…
                </span>
              ) : null}
              <NotificationBadge
                count={backgroundBadgeTotal}
                testId="environment-switcher-badge"
              />
              <ChevronDown
                aria-hidden="true"
                className="h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--text-muted)" }}
              />
            </button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side="bottom">
          {activeSyncing
            ? "Syncing with the host — read-only until it finishes."
            : "Switch environment"}
        </TooltipContent>
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
                presentation={connectionPresentations[environment.id]?.presentation}
                blockedFailure={
                  connectionPresentations[environment.id]?.blockedFailure ?? null
                }
                badgeCount={notificationBadges[environment.id] ?? 0}
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
        {activeEnvironment.kind === "remote" ? (
          <div
            className="mt-1 border-t pt-1"
            style={{ borderColor: "var(--border-subtle)" }}
          >
            <button
              type="button"
              className="flex h-8 w-full items-center rounded-[5px] px-2 text-left text-[0.8125rem] outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
              style={{ color: "var(--text-secondary)" }}
              onClick={() => {
                setOpen(false);
                setJournalOpen(true);
              }}
              data-testid="environment-switcher-connection-log"
            >
              Connection log…
            </button>
          </div>
        ) : null}
      </PopoverContent>
      {activeEnvironment.kind === "remote" ? (
        <RemoteConnectionJournalDialog
          environmentId={activeEnvironment.id}
          environmentName={activeEnvironment.name}
          open={journalOpen}
          onOpenChange={setJournalOpen}
        />
      ) : null}
    </Popover>
  );
});
