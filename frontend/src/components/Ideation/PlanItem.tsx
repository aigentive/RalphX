/**
 * PlanItem - Individual session item in the PlanBrowser sidebar
 *
 * Renders group-specific metadata below the title and context menu
 * actions appropriate for each SessionGroup.
 */

import { memo, useCallback } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  MessageSquare,
  Loader2,
  Clock,
  MoreHorizontal,
  Pencil,
  Archive,
  RotateCcw,
  RefreshCw,
  CircleCheck,
  CheckCircle,
  CornerDownRight,
  ArrowDownToLine,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import { useChatStore, selectAgentStatus } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { formatHumanTimestamp } from "@/lib/formatters";
import type { IdeationSessionWithProgress, SessionProgress } from "@/types/ideation";
import type { SessionGroup } from "./planBrowserUtils";

// ============================================================================
// ActivityIndicator
// ============================================================================

const ACCENT_COLOR = "var(--accent-primary)";
const VERIFYING_COLOR = "var(--status-info)";
const QUEUED_COLOR = "var(--status-warning)";

interface ActivityIndicatorProps {
  isActive: boolean;
  isWaiting: boolean;
  isQueued: boolean;
  label?: string | undefined;
  separator?: string | undefined;
  color?: string | undefined;
}

function ActivityIndicator({ isActive, isWaiting, isQueued, label = "Agent working...", separator, color = ACCENT_COLOR }: ActivityIndicatorProps) {
  if (!isActive && !isWaiting && !isQueued) return null;
  return (
    <>
      {isActive && <span style={{ color }}>{label}</span>}
      {isWaiting && <span style={{ color: "var(--text-muted)" }}>Awaiting input</span>}
      {!isActive && !isWaiting && isQueued && <span style={{ color: QUEUED_COLOR }}>Queued</span>}
      {(isActive || isWaiting) && separator && <span style={{ color: "var(--text-muted)" }}>{separator}</span>}
    </>
  );
}

// ============================================================================
// Types
// ============================================================================

export interface PlanItemProps {
  plan: IdeationSessionWithProgress;
  isSelected: boolean;
  group: SessionGroup;
  isEditing: boolean;
  editingTitle?: string | undefined;
  isMenuOpen: boolean;
  inputRef: React.RefObject<HTMLInputElement | null>;
  onSelect: (planId: string) => void;
  onStartRename: (planId: string, currentTitle: string) => void;
  onConfirmRename: (planId: string) => void;
  onTitleChange: (value: string) => void;
  onKeyDown: (e: React.KeyboardEvent, planId: string) => void;
  onMenuOpenChange: (open: boolean, planId: string) => void;
  onArchive?: (planId: string) => void;
  onReopen?: (planId: string) => void;
  onResetReaccept?: (planId: string) => void;
  onNavigateToSource?: (planId: string) => void;
}

// ============================================================================
// Metadata Line
// ============================================================================

interface MetadataLineProps {
  group: SessionGroup;
  plan: IdeationSessionWithProgress;
  progress: SessionProgress | null;
  isIdeationActive: boolean;
  isIdeationWaiting: boolean;
  isVerifying: boolean;
  isQueued: boolean;
}

function MetadataLine({ group, plan, progress, isIdeationActive, isIdeationWaiting, isVerifying, isQueued }: MetadataLineProps) {
  const parentSessionTitle = plan.parentSessionTitle;
  const activityLabel = isVerifying ? "Verifying..." : undefined;
  const activityColor = isVerifying ? VERIFYING_COLOR : undefined;
  const updatedAtTimestamp = formatHumanTimestamp(plan.updatedAt);
  const convertedAtTimestamp = plan.convertedAt
    ? formatHumanTimestamp(plan.convertedAt)
    : null;

  // Show parent session indicator if this is a child session
  if (parentSessionTitle) {
    return (
      <div
        className="flex flex-col gap-0.5 text-[0.75rem]"
        style={{ color: "var(--text-muted)" }}
      >
        <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} label={activityLabel} color={activityColor} />
        <div className="flex items-center gap-1">
          <CornerDownRight className="w-3 h-3" />
          <span className="truncate">Follow-up of: {parentSessionTitle}</span>
        </div>
      </div>
    );
  }

  switch (group) {
    case "drafts":
      return (
        <div
          className="flex items-center gap-1 text-[0.75rem]"
          style={{ color: "var(--text-muted)" }}
        >
          {(isIdeationActive || isIdeationWaiting || isVerifying || isQueued) ? (
            <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} label={activityLabel} color={activityColor} />
          ) : (
            <>
              <Clock className="w-3 h-3" />
              <span title={updatedAtTimestamp.title || undefined}>
                {updatedAtTimestamp.label}
              </span>
            </>
          )}
        </div>
      );

    case "in-progress":
      if (!progress) {
        if (isIdeationActive || isIdeationWaiting || isVerifying || isQueued) {
          return (
            <span className="text-[0.75rem]">
              <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} label={activityLabel} color={activityColor} />
            </span>
          );
        }
        return null;
      }
      return (
        <div className="flex items-center gap-1 text-[0.75rem]">
          <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} label={activityLabel ?? "Agent working"} separator="·" color={activityColor} />
          <span style={{ color: "var(--status-success)" }}>
            {progress.done}/{progress.total} done
          </span>
          {progress.active > 0 && (
            <>
              <span style={{ color: "var(--text-muted)" }}>&middot;</span>
              <span style={{ color: "var(--accent-primary)" }}>
                {progress.active} active
              </span>
            </>
          )}
        </div>
      );

    case "accepted":
      return (
        <div
          className="flex items-center gap-1 text-[0.75rem]"
          style={{ color: "var(--text-muted)" }}
        >
          <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} separator="·" label={activityLabel} color={activityColor} />
          <span>{progress?.total ?? 0} {(progress?.total ?? 0) === 1 ? "task" : "tasks"}</span>
          {plan.convertedAt && (
            <>
              <span>&middot;</span>
              <span title={convertedAtTimestamp?.title || undefined}>
                {convertedAtTimestamp?.label ?? ""}
              </span>
            </>
          )}
        </div>
      );

    case "done": {
      const hasActivity = isIdeationActive || isIdeationWaiting || isVerifying || isQueued;
      if (!hasActivity) return null;
      return (
        <div
          className="flex items-center gap-1 text-[0.75rem]"
          style={{ color: "var(--text-muted)" }}
        >
          <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} separator="·" label={activityLabel} color={activityColor} />
        </div>
      );
    }

    case "archived": {
      const hasActivity = isIdeationActive || isIdeationWaiting || isVerifying || isQueued;
      if (!hasActivity) return null;
      return (
        <div
          className="flex items-center gap-1 text-[0.75rem]"
          style={{ color: "var(--text-muted)" }}
        >
          <ActivityIndicator isActive={isIdeationActive || isVerifying} isWaiting={isIdeationWaiting} isQueued={isQueued} separator="·" label={activityLabel} color={activityColor} />
        </div>
      );
    }
  }
}

// ============================================================================
// Context Menu
// ============================================================================

const MENU_ITEM_CLASSES = "text-[0.8125rem] cursor-pointer gap-2.5 py-2";

function ContextMenuItems({ group, onStartRename, onArchive, onReopen, onResetReaccept }: {
  group: SessionGroup;
  onStartRename: () => void;
  onArchive?: () => void;
  onReopen?: () => void;
  onResetReaccept?: () => void;
}) {
  return (
    <>
      {/* Rename is available for all groups */}
      <DropdownMenuItem
        onClick={(e) => { e.stopPropagation(); onStartRename(); }}
        className={MENU_ITEM_CLASSES}
      >
        <Pencil className="w-3.5 h-3.5" />
        Rename
      </DropdownMenuItem>

      {group === "drafts" && (
        <DropdownMenuItem
          onClick={(e) => { e.stopPropagation(); onArchive?.(); }}
          className={MENU_ITEM_CLASSES}
        >
          <Archive className="w-3.5 h-3.5" />
          Archive
        </DropdownMenuItem>
      )}

      {(group === "in-progress" || group === "accepted" || group === "done") && (
        <>
          <DropdownMenuItem
            onClick={(e) => { e.stopPropagation(); onReopen?.(); }}
            className={MENU_ITEM_CLASSES}
          >
            <RotateCcw className="w-3.5 h-3.5" />
            Reopen
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={(e) => { e.stopPropagation(); onResetReaccept?.(); }}
            className={MENU_ITEM_CLASSES}
          >
            <RefreshCw className="w-3.5 h-3.5" />
            Reset & Re-accept
          </DropdownMenuItem>
        </>
      )}

      {group === "archived" && (
        <DropdownMenuItem
          onClick={(e) => { e.stopPropagation(); onReopen?.(); }}
          className={MENU_ITEM_CLASSES}
        >
          <RotateCcw className="w-3.5 h-3.5" />
          Reopen
        </DropdownMenuItem>
      )}
    </>
  );
}

// ============================================================================
// Component
// ============================================================================

const isMutedGroup = (group: SessionGroup) => group === "done" || group === "archived";

// Group → leftmost row icon + color. Mirrors SessionGroupHeader's icon so
// expanded collapse items inherit the group's visual identity. Done/Accepted
// punch green via --status-success (matches collapse count badge accent).
const GROUP_ICON: Record<SessionGroup, { Icon: LucideIcon; color: string }> = {
  drafts:        { Icon: MessageSquare, color: "var(--text-muted)" },
  "in-progress": { Icon: Zap,           color: "var(--accent-primary)" },
  accepted:      { Icon: CheckCircle,   color: "var(--status-success)" },
  done:          { Icon: CircleCheck,   color: "var(--status-success)" },
  archived:      { Icon: Archive,       color: "var(--text-muted)" },
};

export const PlanItem = memo(function PlanItem({
  plan,
  isSelected,
  group,
  isEditing,
  editingTitle,
  isMenuOpen,
  inputRef,
  onSelect,
  onStartRename,
  onConfirmRename,
  onTitleChange,
  onKeyDown,
  onMenuOpenChange,
  onArchive,
  onReopen,
  onResetReaccept,
  onNavigateToSource,
}: PlanItemProps) {
  const muted = isMutedGroup(group);
  const storeKey = buildStoreKey("ideation", plan.id);
  const agentStatus = useChatStore(selectAgentStatus(storeKey));
  const isIdeationActive = agentStatus === "generating";
  const isIdeationWaiting = agentStatus === "waiting_for_input";
  const activeVerificationChildId = useIdeationStore(state => state.activeVerificationChildId[plan.id]);
  const isVerifying = (plan.verificationInProgress ?? false) || !!activeVerificationChildId;

  // Internal stable handlers — bind planId so parent callbacks stay stable
  const handleClick = useCallback(() => {
    if (!isEditing) onSelect(plan.id);
  }, [onSelect, plan.id, isEditing]);

  const handleStartRename = useCallback(() => {
    onStartRename(plan.id, plan.title ?? "");
  }, [onStartRename, plan.id, plan.title]);

  const handleConfirmRename = useCallback(() => {
    onConfirmRename(plan.id);
  }, [onConfirmRename, plan.id]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    onKeyDown(e, plan.id);
  }, [onKeyDown, plan.id]);

  const handleMenuOpenChange = useCallback((open: boolean) => {
    onMenuOpenChange(open, plan.id);
  }, [onMenuOpenChange, plan.id]);

  const handleArchive = useCallback(() => {
    onArchive?.(plan.id);
  }, [onArchive, plan.id]);

  const handleReopen = useCallback(() => {
    onReopen?.(plan.id);
  }, [onReopen, plan.id]);

  const handleResetReaccept = useCallback(() => {
    onResetReaccept?.(plan.id);
  }, [onResetReaccept, plan.id]);

  const handleNavigateToSource = useCallback(() => {
    onNavigateToSource?.(plan.id);
  }, [onNavigateToSource, plan.id]);

  return (
    <div
      data-testid={`plan-item-${plan.id}`}
      className={cn(
        "group relative cursor-pointer agents-session-row plan-item-row",
        "mx-2 rounded-[6px]",
        "transition-colors duration-150 ease-out"
      )}
      aria-current={isSelected ? "true" : undefined}
      data-menu-open={isMenuOpen ? "true" : undefined}
      style={{
        padding: "10px 12px",
        opacity: muted && !isSelected ? 0.7 : 1,
      }}
      onClick={handleClick}
    >
      <div className="flex items-center gap-2">
        {/* Plan icon */}
        <div
          className="w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0 transition-colors duration-150"
          style={{
            background: isSelected
              ? withAlpha("var(--accent-primary)", 15)
              : "var(--overlay-faint)",
            border: isSelected
              ? "1px solid var(--accent-border)"
              : "1px solid var(--overlay-faint)",
          }}
        >
          {(isIdeationActive || isVerifying) ? (
            <Loader2
              className="w-3 h-3 animate-spin"
              style={{ color: isVerifying ? VERIFYING_COLOR : ACCENT_COLOR }}
            />
          ) : (() => {
            const { Icon, color } = GROUP_ICON[group];
            const iconColor = isSelected || isIdeationWaiting ? "var(--accent-primary)" : color;
            return <Icon className="w-3 h-3" style={{ color: iconColor }} />;
          })()}
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          {isEditing ? (
            <Input
              ref={inputRef}
              value={editingTitle ?? ""}
              onChange={(e) => onTitleChange(e.target.value)}
              onKeyDown={handleKeyDown}
              onBlur={handleConfirmRename}
              className="h-6 text-[0.8125rem] px-2 py-0 rounded-md"
              style={{
                background: "var(--bg-surface)",
                border: "1px solid var(--overlay-moderate)",
              }}
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <>
              <div className="flex items-center gap-1.5">
                <span
                  className={cn(
                    "text-[0.75rem] font-medium truncate tracking-[-0.01em]",
                    "transition-colors duration-150"
                  )}
                  style={{
                    color: isSelected
                      ? "var(--text-primary)"
                      : muted
                        ? "var(--text-secondary)"
                        : "var(--text-secondary)",
                  }}
                >
                  {plan.title || "Untitled Plan"}
                </span>
                {plan.sourceProjectId && (
                  <button
                    type="button"
                    data-testid="import-badge"
                    className="inline-flex items-center gap-0.5 text-[0.5625rem] font-medium px-1 py-0.5 rounded flex-shrink-0 select-none transition-opacity hover:opacity-80"
                    style={{
                      background: "var(--status-success-muted)",
                      border: "1px solid var(--status-success-border)",
                      color: "var(--status-success)",
                    }}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleNavigateToSource();
                    }}
                    title="Navigate to source session"
                  >
                    <ArrowDownToLine className="w-2 h-2" />
                    Imported
                  </button>
                )}
                {plan.hasPendingPrompt && (
                  <span
                    className="inline-flex items-center justify-center w-2 h-2 rounded-full bg-status-warning animate-pulse flex-shrink-0"
                    title="Waiting to start - message queued"
                  />
                )}
              </div>
              <MetadataLine
                group={group}
                plan={plan}
                progress={plan.progress ?? null}
                isIdeationActive={isIdeationActive}
                isIdeationWaiting={isIdeationWaiting}
                isVerifying={isVerifying}
                isQueued={plan.hasPendingPrompt ?? false}
              />
            </>
          )}
        </div>

        {/* Context Menu */}
        {!isEditing && (
          <DropdownMenu onOpenChange={handleMenuOpenChange}>
            <DropdownMenuTrigger asChild>
              <button
                className={cn(
                  "w-6 h-6 rounded flex items-center justify-center flex-shrink-0",
                  "transition-all duration-150",
                  (isMenuOpen || isSelected)
                    ? "opacity-100"
                    : "opacity-0 group-hover:opacity-100"
                )}
                style={{
                  background: isMenuOpen ? "var(--overlay-weak)" : "transparent",
                }}
                onClick={(e) => e.stopPropagation()}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "var(--overlay-weak)";
                }}
                onMouseLeave={(e) => {
                  if (!isMenuOpen) {
                    e.currentTarget.style.background = "transparent";
                  }
                }}
              >
                <MoreHorizontal className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="w-48"
              style={{
                background: "var(--bg-elevated)",
                border: "1px solid var(--overlay-weak)",
                boxShadow: "var(--shadow-lg)",
              }}
            >
              <ContextMenuItems
                group={group}
                onStartRename={handleStartRename}
                onArchive={handleArchive}
                onReopen={handleReopen}
                onResetReaccept={handleResetReaccept}
              />
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </div>
  );
});
