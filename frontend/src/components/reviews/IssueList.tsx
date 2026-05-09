/**
 * IssueList - Display review issues with severity badges and progress
 *
 * Following macOS Tahoe design:
 * - Flat, neutral backgrounds
 * - Small typography (11-13px)
 * - No shadows or gradients
 */

import { useState, useMemo } from "react";
import {
  AlertCircle,
  AlertTriangle,
  Info,
  Lightbulb,
  ChevronDown,
  ChevronRight,
  FileCode,
  CheckCircle2,
  Clock,
  CircleDot,
  XCircle,
} from "lucide-react";
import type {
  ReviewIssue,
  IssueStatus,
  IssueSeverity,
  IssueCategory,
  IssueProgressSummary,
} from "@/types/review-issue";
import { sortBySeverity } from "@/types/review-issue";

// ============================================================================
// Constants
// ============================================================================

const SEVERITY_CONFIG: Record<
  IssueSeverity,
  { icon: typeof AlertCircle; color: string; bgColor: string; label: string }
> = {
  critical: {
    icon: AlertCircle,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Critical",
  },
  major: {
    icon: AlertTriangle,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Major",
  },
  minor: {
    icon: Info,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Minor",
  },
  suggestion: {
    icon: Lightbulb,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Suggestion",
  },
};

const STATUS_CONFIG: Record<
  IssueStatus,
  { icon: typeof CheckCircle2; color: string; bgColor: string; label: string }
> = {
  open: {
    icon: CircleDot,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Open",
  },
  in_progress: {
    icon: Clock,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "In Progress",
  },
  addressed: {
    icon: CheckCircle2,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Addressed",
  },
  verified: {
    icon: CheckCircle2,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Verified",
  },
  wontfix: {
    icon: XCircle,
    color: "var(--text-muted)",
    bgColor: "var(--overlay-weak)",
    label: "Won't Fix",
  },
};

const CATEGORY_LABELS: Record<IssueCategory, string> = {
  bug: "Bug",
  missing: "Missing",
  quality: "Quality",
  design: "Design",
};

// ============================================================================
// Types
// ============================================================================

export type GroupByOption = "severity" | "status" | "step";

export interface IssueListProps {
  issues: ReviewIssue[];
  groupBy?: GroupByOption;
  showProgress?: boolean;
  progress?: IssueProgressSummary;
  emptyMessage?: string;
  compact?: boolean;
  onIssueClick?: (issue: ReviewIssue) => void;
}

// ============================================================================
// Sub-components
// ============================================================================

interface SeverityBadgeProps {
  severity: IssueSeverity;
  compact?: boolean;
}

function SeverityBadge({ severity, compact = false }: SeverityBadgeProps) {
  const config = SEVERITY_CONFIG[severity];
  const Icon = config.icon;

  return (
    <div
      className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded"
      style={{ backgroundColor: config.bgColor }}
    >
      <Icon className="w-3 h-3" style={{ color: config.color }} />
      {!compact && (
        <span className="text-[0.625rem] font-medium" style={{ color: config.color }}>
          {config.label}
        </span>
      )}
    </div>
  );
}

interface StatusBadgeProps {
  status: IssueStatus;
  compact?: boolean;
}

function StatusBadge({ status, compact = false }: StatusBadgeProps) {
  const config = STATUS_CONFIG[status];
  const Icon = config.icon;

  return (
    <div
      className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded"
      style={{ backgroundColor: config.bgColor }}
    >
      <Icon className="w-3 h-3" style={{ color: config.color }} />
      {!compact && (
        <span className="text-[0.625rem] font-medium" style={{ color: config.color }}>
          {config.label}
        </span>
      )}
    </div>
  );
}

interface IssueCardProps {
  issue: ReviewIssue;
  compact?: boolean | undefined;
  onClick?: (() => void) | undefined;
  isLast?: boolean | undefined;
}

function IssueCard({ issue, compact = false, onClick, isLast = false }: IssueCardProps) {
  const hasFileLink = issue.filePath && issue.lineNumber;

  return (
    <div
      className={`transition-colors ${
        onClick ? "cursor-pointer hover:bg-[var(--overlay-faint)]" : ""
      }`}
      style={!isLast ? { borderBottom: "1px solid var(--border-subtle)" } : undefined}
      onClick={onClick}
    >
      <div className={compact ? "px-3 py-2.5" : "px-3 py-3"}>
        {/* Header row: severity + status + category */}
        <div className="flex items-center gap-2 mb-1.5">
          <SeverityBadge severity={issue.severity} compact={compact} />
          <StatusBadge status={issue.status} compact />
          {issue.category && (
            <span
              className="text-[0.625rem] px-1.5 py-0.5 rounded"
              style={{
                backgroundColor: "var(--bg-hover)",
                color: "var(--text-secondary)",
              }}
            >
              {CATEGORY_LABELS[issue.category]}
            </span>
          )}
        </div>

        {/* Title */}
        <h4
          className={`font-medium ${compact ? "text-[0.75rem]" : "text-[0.8125rem]"}`}
          style={{ color: "var(--text-primary)" }}
        >
          {issue.title}
        </h4>

        {/* Description (if not compact) */}
        {!compact && issue.description && (
          <p
            className="text-[0.75rem] mt-1 line-clamp-2"
            style={{ color: "var(--text-secondary)" }}
          >
            {issue.description}
          </p>
        )}

        {/* File link (if available) */}
        {hasFileLink && (
          <div className="flex items-center gap-1.5 mt-2">
            <FileCode className="w-3 h-3" style={{ color: "var(--text-muted)" }} />
            <span
              className="text-[11px] font-mono"
              style={{ color: "var(--text-muted)" }}
            >
              {issue.filePath}:{issue.lineNumber}
            </span>
          </div>
        )}

        {/* Resolution notes (if addressed/verified) */}
        {!compact && issue.resolutionNotes && (
          <div
            className="mt-2 pt-2"
            style={{ borderTop: "1px solid var(--border-subtle)" }}
          >
            <span
              className="text-[0.625rem] uppercase tracking-wider"
              style={{ color: "var(--text-muted)" }}
            >
              Resolution
            </span>
            <p
              className="text-[0.6875rem] mt-0.5"
              style={{ color: "var(--text-secondary)" }}
            >
              {issue.resolutionNotes}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

interface IssueGroupProps {
  title: string;
  issues: ReviewIssue[];
  compact?: boolean | undefined;
  onIssueClick?: ((issue: ReviewIssue) => void) | undefined;
  defaultExpanded?: boolean | undefined;
}

function IssueGroup({
  title,
  issues,
  compact,
  onIssueClick,
  defaultExpanded = true,
}: IssueGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  if (issues.length === 0) return null;

  return (
    <div className="mb-3">
      {/* Group header */}
      <button
        className="flex items-center gap-1.5 w-full text-left mb-2"
        onClick={() => setIsExpanded(!isExpanded)}
      >
        {isExpanded ? (
          <ChevronDown className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
        ) : (
          <ChevronRight className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
        )}
        <span
          className="text-[0.6875rem] uppercase tracking-wider font-medium"
          style={{ color: "var(--text-muted)" }}
        >
          {title}
        </span>
        <span
          className="text-[0.6875rem] font-medium ml-1"
          style={{ color: "var(--text-muted)" }}
        >
          ({issues.length})
        </span>
      </button>

      {/* Group items */}
      {isExpanded && (
        <div className="pl-5">
          {issues.map((issue, index) => (
            <IssueCard
              key={issue.id}
              issue={issue}
              compact={compact}
              onClick={onIssueClick ? () => onIssueClick(issue) : undefined}
              isLast={index === issues.length - 1}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// IssueProgressBar
// ============================================================================

export interface IssueProgressBarProps {
  progress: IssueProgressSummary;
  showSeverityBreakdown?: boolean;
}

export function IssueProgressBar({
  progress,
  showSeverityBreakdown = false,
}: IssueProgressBarProps) {
  if (progress.total === 0) {
    return null;
  }

  const { total, open, inProgress, addressed, verified, wontfix, percentResolved } =
    progress;

  // Calculate segment widths
  const verifiedWidth = (verified / total) * 100;
  const addressedWidth = (addressed / total) * 100;
  const wontfixWidth = (wontfix / total) * 100;
  const inProgressWidth = (inProgress / total) * 100;
  const openWidth = (open / total) * 100;

  return (
    <div className="space-y-2">
      {/* Progress bar */}
      <div className="flex items-center gap-2">
        <div
          className="flex-1 h-1.5 rounded-full overflow-hidden flex"
          style={{ backgroundColor: "var(--bg-elevated)" }}
        >
          {/* Verified - green solid */}
          {verifiedWidth > 0 && (
            <div
              className="h-full"
              style={{
                width: `${verifiedWidth}%`,
                backgroundColor: "var(--text-muted)",
              }}
            />
          )}
          {/* Addressed - green lighter */}
          {addressedWidth > 0 && (
            <div
              className="h-full"
              style={{
                width: `${addressedWidth}%`,
                backgroundColor: "var(--text-muted)",
              }}
            />
          )}
          {/* Won't fix - gray */}
          {wontfixWidth > 0 && (
            <div
              className="h-full"
              style={{
                width: `${wontfixWidth}%`,
                backgroundColor: "var(--text-muted)",
              }}
            />
          )}
          {/* In progress - yellow */}
          {inProgressWidth > 0 && (
            <div
              className="h-full animate-pulse"
              style={{
                width: `${inProgressWidth}%`,
                backgroundColor: "var(--text-muted)",
              }}
            />
          )}
          {/* Open - blue */}
          {openWidth > 0 && (
            <div
              className="h-full"
              style={{
                width: `${openWidth}%`,
                backgroundColor: "var(--text-muted)",
              }}
            />
          )}
        </div>
        <span
          className="text-[0.625rem] tabular-nums shrink-0"
          style={{ color: "var(--text-muted)" }}
        >
          {Math.round(percentResolved)}%
        </span>
      </div>

      {/* Status counts */}
      <div className="flex items-center gap-3 text-[0.625rem]">
        {verified > 0 && (
          <span style={{ color: "var(--text-muted)" }}>
            {verified} verified
          </span>
        )}
        {addressed > 0 && (
          <span style={{ color: "var(--text-muted)" }}>
            {addressed} addressed
          </span>
        )}
        {inProgress > 0 && (
          <span style={{ color: "var(--text-muted)" }}>
            {inProgress} in progress
          </span>
        )}
        {open > 0 && (
          <span style={{ color: "var(--text-muted)" }}>
            {open} open
          </span>
        )}
        {wontfix > 0 && (
          <span style={{ color: "var(--text-muted)" }}>
            {wontfix} won&apos;t fix
          </span>
        )}
      </div>

      {/* Severity breakdown */}
      {showSeverityBreakdown && (
        <div className="flex items-center gap-3 text-[0.625rem] pt-1">
          {progress.bySeverity.critical.total > 0 && (
            <span style={{ color: SEVERITY_CONFIG.critical.color }}>
              {progress.bySeverity.critical.open}/{progress.bySeverity.critical.total} critical
            </span>
          )}
          {progress.bySeverity.major.total > 0 && (
            <span style={{ color: SEVERITY_CONFIG.major.color }}>
              {progress.bySeverity.major.open}/{progress.bySeverity.major.total} major
            </span>
          )}
          {progress.bySeverity.minor.total > 0 && (
            <span style={{ color: SEVERITY_CONFIG.minor.color }}>
              {progress.bySeverity.minor.open}/{progress.bySeverity.minor.total} minor
            </span>
          )}
          {progress.bySeverity.suggestion.total > 0 && (
            <span style={{ color: SEVERITY_CONFIG.suggestion.color }}>
              {progress.bySeverity.suggestion.open}/{progress.bySeverity.suggestion.total} suggestions
            </span>
          )}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// IssueList Main Component
// ============================================================================

export function IssueList({
  issues,
  groupBy = "severity",
  showProgress = false,
  progress,
  emptyMessage = "No issues found",
  compact = false,
  onIssueClick,
}: IssueListProps) {
  const groupedIssues = useMemo(() => {
    if (groupBy === "severity") {
      const groups: Record<IssueSeverity, ReviewIssue[]> = {
        critical: [],
        major: [],
        minor: [],
        suggestion: [],
      };
      for (const issue of issues) {
        groups[issue.severity].push(issue);
      }
      return [
        { title: "Critical", issues: groups.critical },
        { title: "Major", issues: groups.major },
        { title: "Minor", issues: groups.minor },
        { title: "Suggestions", issues: groups.suggestion },
      ];
    }

    if (groupBy === "status") {
      const groups: Record<IssueStatus, ReviewIssue[]> = {
        open: [],
        in_progress: [],
        addressed: [],
        verified: [],
        wontfix: [],
      };
      for (const issue of issues) {
        groups[issue.status].push(issue);
      }
      // Sort each group by severity
      for (const key of Object.keys(groups) as IssueStatus[]) {
        groups[key] = sortBySeverity(groups[key]);
      }
      return [
        { title: "Open", issues: groups.open },
        { title: "In Progress", issues: groups.in_progress },
        { title: "Addressed", issues: groups.addressed },
        { title: "Verified", issues: groups.verified },
        { title: "Won't Fix", issues: groups.wontfix },
      ];
    }

    // groupBy === "step"
    const byStep = new Map<string | null, ReviewIssue[]>();
    for (const issue of issues) {
      const key = issue.stepId ?? null;
      const existing = byStep.get(key) ?? [];
      existing.push(issue);
      byStep.set(key, existing);
    }
    const result: { title: string; issues: ReviewIssue[] }[] = [];
    for (const [stepId, stepIssues] of byStep) {
      const sorted = sortBySeverity(stepIssues);
      if (stepId === null) {
        // Issues with no step go last
        result.push({ title: "General Issues", issues: sorted });
      } else {
        result.unshift({ title: `Step: ${stepId.slice(0, 8)}...`, issues: sorted });
      }
    }
    return result;
  }, [issues, groupBy]);

  if (issues.length === 0) {
    return (
      <p className="text-[0.75rem] italic py-2" style={{ color: "var(--text-muted)" }}>
        {emptyMessage}
      </p>
    );
  }

  return (
    <div>
      {/* Progress bar */}
      {showProgress && progress && (
        <div className="mb-4">
          <IssueProgressBar progress={progress} showSeverityBreakdown />
        </div>
      )}

      {/* Issue groups */}
      {groupedIssues.map(({ title, issues: groupIssues }) => (
        <IssueGroup
          key={title}
          title={title}
          issues={groupIssues}
          compact={compact}
          onIssueClick={onIssueClick}
        />
      ))}
    </div>
  );
}

// Export sub-components for direct use
export { SeverityBadge, StatusBadge, IssueCard, IssueGroup };
