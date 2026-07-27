/**
 * PlanDisplay - macOS Tahoe styled plan artifact display
 *
 * Design: Glass-morphism collapsible with refined typography,
 * warm orange accent for actions, and smooth animations.
 */

import {
  useState,
  useCallback,
  useEffect,
  useId,
  type ReactNode,
} from "react";
import { toast } from "sonner";
import { FileEdit, Download, CheckCircle2, ChevronDown, FileText, Sparkles, History, Loader2, ArrowLeft, ListPlus, MoreHorizontal, Copy, ShieldCheck, Rocket, MessageSquarePlus } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { artifactApi } from "@/api/artifact";
import type { Artifact, ArtifactVersionSummary } from "@/types/artifact";
import { formatDateTime } from "@/lib/formatters";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import { ArtifactSelectableRegion } from "@/components/agents/artifact-selection/ArtifactSelectableRegion";
import {
  PlanBundleTabs,
  type PlanBundleBodyMode,
} from "./PlanBundleTabs";
import {
  planBundlePanelId,
  planBundleTabId,
} from "./planBundleTabIds";

// ============================================================================
// Types
// ============================================================================

export interface PlanDisplayConversationReference {
  artifactId: string;
  title: string;
  version: number;
}

export type PlanDisplayBodyMode = PlanBundleBodyMode;

export interface PlanDisplayProps {
  plan: Artifact;
  artifactLabel?: string;
  showApprove?: boolean;
  linkedProposalsCount?: number;
  bodyMode?: PlanDisplayBodyMode;
  onBodyModeChange?: (mode: PlanDisplayBodyMode) => void;
  bodyTabsIdPrefix?: string;
  hideBody?: boolean;
  onEdit?: () => void;
  onExport?: () => void;
  onStartNewConversationWithPlan?: (
    reference: PlanDisplayConversationReference,
  ) => void;
  /** A v2 bundle cannot safely copy a historical overview without its paired blueprint. */
  disableHistoricalNewConversation?: boolean;
  onApprove?: () => void;
  isApproving?: boolean;
  approveLabel?: string;
  isApproved?: boolean;
  onVerifyPlan?: () => void;
  isVerifyingPlan?: boolean;
  verifyPlanLabel?: string;
  /** Controlled expanded state - if provided, component is controlled */
  isExpanded?: boolean;
  /** Callback when expanded state changes */
  onExpandedChange?: (expanded: boolean) => void;
  /** @deprecated No longer used - version selection is now inline. Will be removed in Task 2. */
  onViewHistory?: () => void;
  /** When set, navigates PlanDisplay to this version number */
  requestedVersion?: number;
  /** Called after requestedVersion has been applied, to clear the parent's state */
  onVersionViewed?: () => void;
  /** Called when user clicks the proposal action — triggers orchestrator to decompose plan into tasks */
  onCreateProposals?: () => void;
  createProposalsLabel?: string;
  /** Called when user chooses to implement the approved plan directly in the agent workspace */
  onImplementDirectly?: () => void;
  implementDirectlyLabel?: string;
  isImplementingDirectly?: boolean;
  primaryPlanAction?: "implement_directly" | "create_proposals";
  isPlanActionRecommendationPending?: boolean;
  planActionHint?: string | null;
  /** Show the overflow action cluster (version picker / copy / export / edit) */
  showOverflowActions?: boolean;
  /** Domain-specific actions rendered in the shared chromeless action row. */
  artifactActions?: ReactNode;
  /** Disable excerpt capture when the artifact has no supported composer reference kind. */
  excerptSelectionEnabled?: boolean;
  /** Separates artifact-specific metadata from the Markdown body for every selected version. */
  prepareContent?: (content: string) => {
    content: string;
    preamble?: ReactNode;
  };
  /**
   * When true, drop the wrapper card (file-icon header + collapsible chrome)
   * and render the markdown body directly. The leading H1 in the plan content
   * already serves as the title in the body, so the wrapper card is
   * redundant in hosts that always show one plan per session.
   */
  chromeless?: boolean;
}

// ============================================================================
// Markdown Components — theme-aware typography
//
// Tokens only — no hard-coded white/[0.0X] alphas. Light/HC themes override
// --overlay-*, --border-*, --text-*, --accent-* so the same classes reskin
// cleanly in all three palettes.
// ============================================================================

const markdownComponents = {
  a: ({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="underline underline-offset-[3px] transition-colors duration-150 hover:brightness-110"
      style={{
        color: "var(--accent-primary)",
        textDecorationColor: "var(--accent-border)",
        textDecorationThickness: "1px",
      }}
      {...props}
    >
      {children}
    </a>
  ),
  code: ({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.includes("language-");
    if (isBlock) {
      return (
        <code
          className={cn(
            "block p-4 rounded-lg text-[0.7812rem] font-mono overflow-x-auto leading-[1.6]",
            className
          )}
          style={{
            background: "var(--overlay-faint)",
            border: "1px solid var(--border-subtle)",
            color: "var(--text-primary)",
          }}
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code
        className="px-[6px] py-[2px] rounded-[4px] text-[0.75rem] font-mono"
        style={{
          background: "var(--overlay-moderate)",
          color: "var(--text-primary)",
        }}
        {...props}
      >
        {children}
      </code>
    );
  },
  pre: ({ children, ...props }: React.HTMLAttributes<HTMLPreElement>) => (
    <pre className="my-4 rounded-lg overflow-hidden" {...props}>
      {children}
    </pre>
  ),
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
    <p
      className="mb-3.5 last:mb-0 leading-[1.65] text-[0.8125rem]"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </p>
  ),
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul
      className="mb-4 last:mb-0 space-y-1.5 pl-5 list-disc marker:text-[var(--text-muted)]"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol
      className="mb-4 last:mb-0 space-y-1.5 pl-6 list-decimal marker:font-semibold marker:text-[0.75rem]"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </ol>
  ),
  li: ({ children, ...props }: React.LiHTMLAttributes<HTMLLIElement>) => (
    <li
      className="leading-[1.65] text-[0.8125rem] pl-1"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </li>
  ),
  // H1 — plan title. Large, tight tracking, accent-tinted bottom divider.
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1
      className="text-[1.25rem] font-semibold tracking-[-0.02em] mt-0 mb-5 pb-3 first:mt-0"
      style={{
        color: "var(--text-primary)",
        borderBottom: "1px solid var(--border-subtle)",
      }}
      {...props}
    >
      {children}
    </h1>
  ),
  // H2 — major section.
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2
      className="text-[0.9375rem] font-semibold tracking-[-0.015em] mt-5 mb-2 first:mt-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h2>
  ),
  // H3 — subsection. Compact uppercase eyebrow in muted text color.
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3
      className="text-[0.6875rem] font-semibold uppercase mt-5 mb-2"
      style={{
        color: "var(--text-muted)",
        letterSpacing: "0.08em",
      }}
      {...props}
    >
      {children}
    </h3>
  ),
  h4: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h4
      className="text-[0.8125rem] font-semibold mt-4 mb-1.5"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h4>
  ),
  blockquote: ({ children, ...props }: React.HTMLAttributes<HTMLQuoteElement>) => (
    <blockquote
      className="my-4 px-3 py-2 rounded-md"
      style={{
        background: "var(--overlay-faint)",
        color: "var(--text-secondary)",
      }}
      {...props}
    >
      {children}
    </blockquote>
  ),
  hr: ({ ...props }: React.HTMLAttributes<HTMLHRElement>) => (
    <hr
      className="my-6"
      style={{
        border: "none",
        borderTop: "1px solid var(--border-subtle)",
      }}
      {...props}
    />
  ),
  strong: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <strong
      className="font-semibold"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </strong>
  ),
  em: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <em
      className="italic"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </em>
  ),
  // Table (GFM)
  table: ({ children, ...props }: React.TableHTMLAttributes<HTMLTableElement>) => (
    <div
      className="my-4 overflow-x-auto rounded-lg"
      style={{ border: "1px solid var(--border-subtle)" }}
    >
      <table className="w-full text-[0.7812rem] border-collapse" {...props}>
        {children}
      </table>
    </div>
  ),
  thead: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <thead
      style={{
        background: "var(--overlay-faint)",
        borderBottom: "1px solid var(--border-subtle)",
      }}
      {...props}
    >
      {children}
    </thead>
  ),
  tbody: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <tbody {...props}>{children}</tbody>
  ),
  tr: ({ children, ...props }: React.HTMLAttributes<HTMLTableRowElement>) => (
    <tr
      className="last:border-b-0"
      style={{ borderBottom: "1px solid var(--border-subtle)" }}
      {...props}
    >
      {children}
    </tr>
  ),
  th: ({ children, ...props }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
    <th
      className="px-3.5 py-2.5 text-left text-[0.6875rem] font-semibold uppercase"
      style={{
        color: "var(--text-primary)",
        letterSpacing: "0.05em",
      }}
      {...props}
    >
      {children}
    </th>
  ),
  td: ({ children, ...props }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
    <td
      className="px-3.5 py-2.5 leading-[1.6] align-top"
      style={{ color: "var(--text-secondary)" }}
      {...props}
    >
      {children}
    </td>
  ),
};

interface ArtifactMarkdownBodyProps {
  content: string;
  source: React.ComponentProps<typeof ArtifactSelectableRegion>["source"];
  excerptSelectionEnabled: boolean;
  className: string;
}

function ArtifactMarkdownBody({
  content,
  source,
  excerptSelectionEnabled,
  className,
}: ArtifactMarkdownBodyProps) {
  const markdown = (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
      {content}
    </ReactMarkdown>
  );

  return excerptSelectionEnabled ? (
    <ArtifactSelectableRegion source={source} className={className}>
      {markdown}
    </ArtifactSelectableRegion>
  ) : (
    <div className={className}>{markdown}</div>
  );
}

// ============================================================================
// Component
// ============================================================================

export function PlanDisplay({
  plan,
  artifactLabel = "Plan",
  showApprove = false,
  linkedProposalsCount = 0,
  bodyMode = "overview",
  onBodyModeChange,
  bodyTabsIdPrefix,
  hideBody = false,
  onEdit,
  onExport,
  onStartNewConversationWithPlan,
  disableHistoricalNewConversation = false,
  onApprove,
  isApproving = false,
  approveLabel = "Approve Plan",
  isApproved = false,
  onVerifyPlan,
  isVerifyingPlan = false,
  verifyPlanLabel = "Verify Plan",
  isExpanded,
  onExpandedChange,
  requestedVersion,
  onVersionViewed,
  onCreateProposals,
  createProposalsLabel = "Create Proposals",
  onImplementDirectly,
  implementDirectlyLabel = "Implement Directly",
  isImplementingDirectly = false,
  primaryPlanAction,
  isPlanActionRecommendationPending = false,
  planActionHint = null,
  showOverflowActions = true,
  artifactActions,
  excerptSelectionEnabled = true,
  prepareContent,
  chromeless = false,
}: PlanDisplayProps) {
  // Use controlled state if isExpanded prop is provided, otherwise use internal state
  // Default to collapsed (false) for initial render
  const [internalIsOpen, setInternalIsOpen] = useState(false);

  const showCreateProposals =
    onCreateProposals &&
    (linkedProposalsCount === undefined || linkedProposalsCount === 0);
  const showPlanBodyTabs = onBodyModeChange !== undefined;
  const generatedBodyTabsId = useId();
  const resolvedBodyTabsIdPrefix =
    bodyTabsIdPrefix ?? `plan-bundle-${generatedBodyTabsId.replace(/:/g, "")}`;
  const showImplementDirectly = Boolean(onImplementDirectly);
  const isCreateProposalsPrimary =
    !isPlanActionRecommendationPending &&
    (primaryPlanAction === "create_proposals" ||
      (!showImplementDirectly && Boolean(showCreateProposals)));
  const isImplementDirectlyPrimary =
    !isPlanActionRecommendationPending &&
    (primaryPlanAction === "implement_directly" ||
      (showImplementDirectly && !showCreateProposals));
  const isOpen = isExpanded !== undefined ? isExpanded : internalIsOpen;
  const setIsOpen = onExpandedChange ?? setInternalIsOpen;
  const actionButtonStyle = (isPrimary: boolean) => ({
    color: isPrimary ? "var(--accent-primary)" : "var(--text-secondary)",
    background: isPrimary ? withAlpha("var(--accent-primary)", 10) : "transparent",
    border: isPrimary ? "1px solid var(--accent-border)" : "1px solid var(--border-subtle)",
  });
  const actionButtonHover = (
    e: React.MouseEvent<HTMLButtonElement>,
    isPrimary: boolean,
  ) => {
    e.currentTarget.style.background = isPrimary
      ? withAlpha("var(--accent-primary)", 15)
      : "var(--overlay-faint)";
  };
  const actionButtonLeave = (
    e: React.MouseEvent<HTMLButtonElement>,
    isPrimary: boolean,
  ) => {
    e.currentTarget.style.background = isPrimary
      ? withAlpha("var(--accent-primary)", 10)
      : "transparent";
  };

  // Version selector state
  const [selectedVersion, setSelectedVersion] = useState(plan.metadata.version);
  const [historicalContent, setHistoricalContent] = useState<string | null>(null);
  const [isLoadingVersion, setIsLoadingVersion] = useState(false);
  const [isVersionDropdownOpen, setIsVersionDropdownOpen] = useState(false);
  const [versionHistory, setVersionHistory] = useState<ArtifactVersionSummary[] | null>(null);

  // Reset to latest when plan changes (new artifact or version update)
  useEffect(() => {
    setSelectedVersion(plan.metadata.version);
    setHistoricalContent(null);
    setVersionHistory(null);
  }, [plan.id, plan.metadata.version]);

  // Fetch version history on plan load (when multi-version)
  useEffect(() => {
    if (plan.metadata.version <= 1) return;

    let cancelled = false;
    artifactApi.getVersionHistory(plan.id)
      .then((history) => {
        if (!cancelled) setVersionHistory(history);
      })
      .catch(() => {
        // Graceful fallback: render without timestamps
        if (!cancelled) setVersionHistory(null);
      });

    return () => { cancelled = true; };
  }, [plan.id, plan.metadata.version]);

  // Fetch historical version when selection changes
  useEffect(() => {
    if (selectedVersion === plan.metadata.version) {
      setHistoricalContent(null);
      setIsLoadingVersion(false);
      return;
    }

    let cancelled = false;
    setIsLoadingVersion(true);

    artifactApi.getAtVersion(plan.id, selectedVersion)
      .then((artifact) => {
        if (cancelled) return;
        if (artifact?.content.type === "inline") {
          setHistoricalContent(artifact.content.text);
        } else {
          setHistoricalContent(null);
        }
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to fetch historical version:", err);
        setHistoricalContent(null);
      })
      .finally(() => {
        if (!cancelled) setIsLoadingVersion(false);
      });

    return () => { cancelled = true; };
  }, [plan.id, selectedVersion, plan.metadata.version]);

  const planContent = plan.content.type === "inline" ? plan.content.text : "";
  const displayContent = historicalContent ?? planContent;
  const preparedContent = prepareContent?.(displayContent);
  const renderedContent = preparedContent?.content ?? displayContent;
  const artifactPreamble = preparedContent?.preamble;
  const isViewingHistorical = selectedVersion !== plan.metadata.version;
  const currentBodyMode = bodyMode ?? "overview";

  const handleBackToLatest = useCallback(() => {
    setSelectedVersion(plan.metadata.version);
  }, [plan.metadata.version]);

  const handleVersionSelect = useCallback((version: number) => {
    setSelectedVersion(version);
    // Auto-expand when selecting a version
    if (!isOpen) {
      setIsOpen(true);
    }
  }, [isOpen, setIsOpen]);

  // When a parent requests a specific version (e.g., from "View plan as of proposal creation"),
  // select that version and then notify the parent so it can clear the request.
  useEffect(() => {
    if (requestedVersion === undefined || requestedVersion === null) return;
    if (requestedVersion !== selectedVersion) {
      handleVersionSelect(requestedVersion);
    }
    onVersionViewed?.();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestedVersion]);

  const handleExport = useCallback(() => {
    if (onExport) {
      onExport();
      return;
    }

    const blob = new Blob([planContent], { type: "text/markdown" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${plan.name.replace(/[^a-z0-9]/gi, "_").toLowerCase()}.md`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }, [planContent, plan.name, onExport]);

  const handleStartNewConversationWithPlan = useCallback(() => {
    onStartNewConversationWithPlan?.({
      artifactId: plan.id,
      title: plan.name,
      version: selectedVersion,
    });
  }, [
    onStartNewConversationWithPlan,
    plan.id,
    plan.name,
    selectedVersion,
  ]);

  if (chromeless) {
    // Extract the leading H1 from the plan body so we can render it on
    // the same row as the action cluster instead of having the title
    // and actions stack in awkwardly disconnected rows.
    const headingMatch = renderedContent
      ? renderedContent.match(/^[ \t]*#\s+(.+?)\s*\n/)
      : null;
    const headingTitle = headingMatch?.[1] ?? null;
    const bodyAfterHeading = headingMatch
      ? renderedContent.slice(headingMatch[0].length)
      : renderedContent;

    const hasActionCTAs =
      !isViewingHistorical &&
      ((showApprove && !isApproved) ||
        isApproved ||
        showPlanBodyTabs ||
        onVerifyPlan ||
        showCreateProposals ||
        showImplementDirectly ||
        Boolean(artifactActions));
    const body = isLoadingVersion ? (
      <div className="flex items-center justify-center py-12">
        <Loader2
          className="w-6 h-6 animate-spin"
          style={{ color: "var(--accent-primary)" }}
        />
      </div>
    ) : renderedContent ? (
      <ArtifactMarkdownBody
        content={bodyAfterHeading ?? ""}
        source={{
          sourceKind: artifactLabel === "Review" ? "review" : "plan",
          sourceId: plan.id,
          sourceLabel: artifactLabel,
          title: plan.name,
          artifactId: plan.id,
          version: selectedVersion,
        }}
        excerptSelectionEnabled={excerptSelectionEnabled}
        className="text-[0.8125rem] leading-relaxed"
      />
    ) : (
      <p
        className="text-[0.8125rem] italic py-8 text-center"
        style={{ color: "var(--text-muted)" }}
      >
        No content available
      </p>
    );

    return (
      <div data-testid="plan-display-chromeless" className="group">
        <div className="pb-2 mb-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
          <div className="flex items-center justify-between gap-3">
            <h1
              className="text-[1.125rem] font-semibold tracking-[-0.02em] flex-1 min-w-0 leading-tight"
              style={{ color: "var(--text-primary)" }}
            >
              {headingTitle ?? plan.name}
            </h1>
            <div className="flex items-center gap-1 shrink-0">
              {showOverflowActions && plan.metadata.version > 1 && (
                <DropdownMenu open={isVersionDropdownOpen} onOpenChange={setIsVersionDropdownOpen}>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-[0.6875rem] gap-1 rounded-lg transition-colors duration-150"
                      style={{ color: "var(--text-muted)" }}
                      title="View version history"
                    >
                      <History className="w-3 h-3" />
                      v{selectedVersion}
                      <ChevronDown className="w-3 h-3" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="w-72"
                    style={{
                      background: "var(--bg-elevated)",
                      backdropFilter: "blur(20px)",
                      border: "1px solid var(--overlay-weak)",
                      boxShadow: "var(--shadow-lg)",
                    }}
                  >
                    {Array.from({ length: plan.metadata.version }, (_, i) => plan.metadata.version - i).map((version) => {
                      const isSelected = version === selectedVersion;
                      const isLatest = version === plan.metadata.version;
                      const versionSummary = versionHistory?.find((v) => v.version === version);
                      const timestamp = versionSummary ? formatDateTime(versionSummary.created_at) : null;
                      return (
                        <DropdownMenuItem
                          key={version}
                          onClick={() => handleVersionSelect(version)}
                          className="text-[0.75rem] cursor-pointer px-3 py-2"
                          style={{
                            background: isSelected ? withAlpha("var(--accent-primary)", 10) : "transparent",
                            borderLeft: isSelected ? "2px solid var(--accent-primary)" : "2px solid transparent",
                          }}
                        >
                          <span className="flex items-center gap-2 w-full">
                            {isSelected && (
                              <span
                                className="w-1.5 h-1.5 rounded-full"
                                style={{ background: "var(--accent-primary)" }}
                              />
                            )}
                            <span>
                              v{version}
                              {timestamp && (
                                <span style={{ color: "var(--text-muted)" }}> — {timestamp}</span>
                              )}
                            </span>
                            {isLatest && (
                              <span className="ml-auto" style={{ color: "var(--text-muted)" }}>
                                (latest)
                              </span>
                            )}
                          </span>
                        </DropdownMenuItem>
                      );
                    })}
                  </DropdownMenuContent>
                </DropdownMenu>
              )}

              {showOverflowActions && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 w-7 p-0 rounded-lg transition-colors duration-150"
                      style={{ color: "var(--text-muted)" }}
                      aria-label={`${artifactLabel} actions`}
                    >
                      <MoreHorizontal className="w-3.5 h-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="w-44"
                    style={{
                      background: "var(--bg-elevated)",
                      backdropFilter: "blur(20px)",
                      border: "1px solid var(--overlay-weak)",
                      boxShadow: "var(--shadow-lg)",
                    }}
                  >
                    <DropdownMenuItem
                      onClick={() => {
                        const content = plan.content.type === "inline" ? plan.content.text : "";
                        navigator.clipboard.writeText(content).then(() => {
                          toast.success("Copied to clipboard");
                        }).catch(() => {
                          toast.error("Failed to copy");
                        });
                      }}
                      disabled={plan.content.type !== "inline" || !plan.content.text}
                      className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                    >
                      <Copy className="w-3.5 h-3.5" />
                      Copy Markdown
                    </DropdownMenuItem>
                    {onStartNewConversationWithPlan &&
                      (!disableHistoricalNewConversation || !isViewingHistorical) && (
                      <DropdownMenuItem
                        onClick={handleStartNewConversationWithPlan}
                        className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                      >
                        <MessageSquarePlus className="w-3.5 h-3.5" />
                        New Conversation
                      </DropdownMenuItem>
                    )}
                    {onEdit && (
                      <DropdownMenuItem
                        onClick={onEdit}
                        className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                      >
                        <FileEdit className="w-3.5 h-3.5" />
                        Edit
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem
                      onClick={handleExport}
                      className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                    >
                      <Download className="w-3.5 h-3.5" />
                      Export...
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>
          </div>

          {hasActionCTAs && (
            <div className="flex flex-wrap items-center gap-1.5 mt-2.5">
              {showApprove && !isApproved && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onApprove}
                  disabled={isApproving}
                  data-testid="plan-approve-button"
                  className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                  style={{
                    color: "var(--accent-primary)",
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 15);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
                  }}
                >
                  <Sparkles className="w-3 h-3" />
                  {isApproving ? "Approving..." : approveLabel}
                </Button>
              )}

              {isApproved && (
                <span
                  className="flex items-center gap-1.5 text-[0.6875rem] font-medium px-2.5 py-1 rounded-lg"
                  style={{
                    background: "var(--status-success-muted)",
                    border: "1px solid var(--status-success-border)",
                    color: "var(--status-success)",
                  }}
                >
                  <CheckCircle2 className="w-3 h-3" />
                  Plan Approved
                </span>
              )}

              {!isViewingHistorical && artifactActions}

              {showPlanBodyTabs && (
                <PlanBundleTabs
                  idPrefix={resolvedBodyTabsIdPrefix}
                  value={currentBodyMode}
                  onValueChange={(mode) => onBodyModeChange?.(mode)}
                  linkedProposalsCount={linkedProposalsCount}
                />
              )}

              {onVerifyPlan && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onVerifyPlan}
                  disabled={
                    isVerifyingPlan || isPlanActionRecommendationPending
                  }
                  data-testid="plan-verify-button"
                  className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                  style={{
                    color: "var(--accent-primary)",
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 15);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
                  }}
                >
                  {isVerifyingPlan ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <ShieldCheck className="w-3 h-3" />
                  )}
                  {isVerifyingPlan ? "Verifying..." : verifyPlanLabel}
                </Button>
              )}

              {isPlanActionRecommendationPending ||
              primaryPlanAction === "create_proposals" ? (
                <>
                  {showCreateProposals && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={onCreateProposals}
                      disabled={isPlanActionRecommendationPending}
                      className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                      style={actionButtonStyle(isCreateProposalsPrimary)}
                      onMouseEnter={(e) => actionButtonHover(e, isCreateProposalsPrimary)}
                      onMouseLeave={(e) => actionButtonLeave(e, isCreateProposalsPrimary)}
                    >
                      <ListPlus className="w-3 h-3" />
                      {createProposalsLabel}
                    </Button>
                  )}
                  {showImplementDirectly && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={onImplementDirectly}
                      disabled={
                        isImplementingDirectly ||
                        isPlanActionRecommendationPending
                      }
                      className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                      style={actionButtonStyle(isImplementDirectlyPrimary)}
                      onMouseEnter={(e) => actionButtonHover(e, isImplementDirectlyPrimary)}
                      onMouseLeave={(e) => actionButtonLeave(e, isImplementDirectlyPrimary)}
                    >
                      {isImplementingDirectly ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Rocket className="w-3 h-3" />
                      )}
                      {isImplementingDirectly ? "Starting..." : implementDirectlyLabel}
                    </Button>
                  )}
                </>
              ) : (
                <>
                  {showImplementDirectly && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={onImplementDirectly}
                      disabled={
                        isImplementingDirectly ||
                        isPlanActionRecommendationPending
                      }
                      className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                      style={actionButtonStyle(isImplementDirectlyPrimary)}
                      onMouseEnter={(e) =>
                        actionButtonHover(e, isImplementDirectlyPrimary)
                      }
                      onMouseLeave={(e) =>
                        actionButtonLeave(e, isImplementDirectlyPrimary)
                      }
                    >
                      {isImplementingDirectly ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Rocket className="w-3 h-3" />
                      )}
                      {isImplementingDirectly ? "Starting..." : implementDirectlyLabel}
                    </Button>
                  )}
                  {showCreateProposals && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={onCreateProposals}
                      disabled={isPlanActionRecommendationPending}
                      className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                      style={actionButtonStyle(isCreateProposalsPrimary)}
                      onMouseEnter={(e) =>
                        actionButtonHover(e, isCreateProposalsPrimary)
                      }
                      onMouseLeave={(e) =>
                        actionButtonLeave(e, isCreateProposalsPrimary)
                      }
                    >
                      <ListPlus className="w-3 h-3" />
                      {createProposalsLabel}
                    </Button>
                  )}
                </>
              )}
            </div>
          )}

          {planActionHint && (
            <p
              className="mt-2 text-[0.6875rem] leading-snug"
              style={{ color: "var(--text-muted)" }}
            >
              {planActionHint}
            </p>
          )}
        </div>

        {/* Version banner when viewing historical */}
        {isViewingHistorical && (
          <div
            className="flex items-center justify-between mb-4 px-3 py-2.5 rounded-lg"
            style={{
              background: "var(--status-warning-muted)",
              border: "1px solid var(--status-warning-border)",
            }}
          >
            <span className="text-[0.75rem] font-medium" style={{ color: "var(--status-warning)" }}>
              Viewing version {selectedVersion} of {plan.metadata.version}
            </span>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleBackToLatest}
              className="h-6 px-2.5 text-[0.6875rem] font-medium gap-1.5 rounded-md transition-colors duration-150"
              style={{
                background: "var(--status-warning-muted)",
                color: "var(--status-warning)",
              }}
            >
              <ArrowLeft className="w-3 h-3" />
              Back to latest
            </Button>
          </div>
        )}

        {!isLoadingVersion && artifactPreamble}

        {hideBody ? null : showPlanBodyTabs ? (
          <div
            id={planBundlePanelId(
              resolvedBodyTabsIdPrefix,
              currentBodyMode,
            )}
            role="tabpanel"
            aria-labelledby={planBundleTabId(
              resolvedBodyTabsIdPrefix,
              currentBodyMode,
            )}
            tabIndex={0}
          >
            {body}
          </div>
        ) : (
          body
        )}
      </div>
    );
  }

  return (
    <div
      className="group"
    >
      <Collapsible open={isOpen} onOpenChange={setIsOpen}>
        {/* Header - flat Tahoe style */}
        <div
          data-testid="plan-card-header"
          className="rounded-xl transition-all duration-200"
          style={{
            padding: "12px 14px",
            background: isOpen
              ? withAlpha("var(--accent-primary)", 8)
              : "var(--overlay-faint)",
            border: isOpen
              ? "1px solid var(--accent-border)"
              : "1px solid var(--overlay-faint)",
          }}
        >
          <div className="flex items-center gap-3">
            <CollapsibleTrigger asChild>
              <button
                className="flex items-center gap-3 text-left flex-1 min-w-0"
              >
                {/* Icon - flat style */}
                <div
                  className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 transition-all duration-200"
                  style={{
                    background: isOpen
                      ? withAlpha("var(--accent-primary)", 15)
                      : "var(--overlay-faint)",
                    border: isOpen
                      ? "1px solid var(--accent-border)"
                      : "1px solid var(--overlay-faint)",
                  }}
                >
                  <FileText
                    className="w-4 h-4 transition-colors duration-200"
                    style={{ color: isOpen ? "var(--accent-primary)" : "var(--text-muted)" }}
                  />
                </div>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span
                      className="text-[0.8125rem] font-medium truncate tracking-[-0.01em]"
                      style={{ color: "var(--text-primary)" }}
                    >
                      {plan.name}
                    </span>

                    <span
                      className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md flex-shrink-0"
                      style={{
                        background: "var(--overlay-faint)",
                        border: "1px solid var(--overlay-faint)",
                        color: "var(--text-muted)",
                      }}
                    >
                      v{plan.metadata.version}
                    </span>

                  </div>

                  {linkedProposalsCount > 0 && (
                    <span
                      className="text-[0.6875rem] mt-0.5 block"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {linkedProposalsCount} linked proposal{linkedProposalsCount !== 1 ? "s" : ""}
                    </span>
                  )}
                </div>

                <ChevronDown
                  className={cn(
                    "w-4 h-4 transition-transform duration-200 flex-shrink-0",
                    !isOpen && "-rotate-90"
                  )}
                  style={{ color: "var(--text-muted)" }}
                />
              </button>
            </CollapsibleTrigger>

            {/* Actions - appear on hover (CSS group-hover), always visible when expanded or version dropdown is open */}
            <div className={cn(
              "flex items-center gap-1 transition-opacity duration-150",
              isVersionDropdownOpen || isOpen ? "opacity-100" : "opacity-0 group-hover:opacity-100"
            )}>
              {/* Approve — only when plan approval is enabled */}
              {showApprove && !isApproved && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onApprove}
                  disabled={isApproving}
                  data-testid="plan-approve-button"
                  className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                  style={{
                    color: "var(--accent-primary)",
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 15);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
                  }}
                >
                  <Sparkles className="w-3 h-3" />
                  {isApproving ? "Approving..." : approveLabel}
                </Button>
              )}

              {isApproved && (
                <span
                  className="flex items-center gap-1.5 text-[0.6875rem] font-medium px-2.5 py-1 rounded-lg"
                  style={{
                    background: "var(--status-success-muted)",
                    border: "1px solid var(--status-success-border)",
                    color: "var(--status-success)",
                  }}
                >
                  <CheckCircle2 className="w-3 h-3" />
                  Plan Approved
                </span>
              )}

              {onVerifyPlan && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onVerifyPlan}
                  disabled={isVerifyingPlan}
                  data-testid="plan-verify-button"
                  className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                  style={{
                    color: "var(--accent-primary)",
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 15);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
                  }}
                >
                  {isVerifyingPlan ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <ShieldCheck className="w-3 h-3" />
                  )}
                  {isVerifyingPlan ? "Verifying..." : verifyPlanLabel}
                </Button>
              )}

              {showCreateProposals && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={onCreateProposals}
                  className="h-7 px-2.5 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-colors duration-150"
                  style={{
                    color: "var(--accent-primary)",
                    background: withAlpha("var(--accent-primary)", 10),
                    border: "1px solid var(--accent-border)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 15);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
                  }}
                >
                  <ListPlus className="w-3 h-3" />
                  {createProposalsLabel}
                </Button>
              )}

              {showOverflowActions && plan.metadata.version > 1 && (
                <DropdownMenu open={isVersionDropdownOpen} onOpenChange={setIsVersionDropdownOpen}>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-[0.6875rem] gap-1 rounded-lg transition-colors duration-150"
                      style={{ color: "var(--text-muted)" }}
                      title="View version history"
                      onMouseEnter={(e) => {
                        e.currentTarget.style.background = "var(--overlay-weak)";
                        e.currentTarget.style.color = "var(--text-primary)";
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.background = "transparent";
                        e.currentTarget.style.color = "var(--text-muted)";
                      }}
                      aria-label="View version history"
                    >
                      <History className="w-3 h-3" />
                      v{selectedVersion}
                      <ChevronDown className="w-3 h-3" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="w-72"
                    style={{
                      background: "var(--bg-elevated)",
                      backdropFilter: "blur(20px)",
                      border: "1px solid var(--overlay-weak)",
                      boxShadow: "var(--shadow-lg)",
                    }}
                  >
                    {Array.from({ length: plan.metadata.version }, (_, i) => plan.metadata.version - i).map((version) => {
                      const isSelected = version === selectedVersion;
                      const isLatest = version === plan.metadata.version;
                      const versionSummary = versionHistory?.find((v) => v.version === version);
                      const timestamp = versionSummary ? formatDateTime(versionSummary.created_at) : null;
                      return (
                        <DropdownMenuItem
                          key={version}
                          onClick={() => handleVersionSelect(version)}
                          className="text-[0.75rem] cursor-pointer px-3 py-2"
                          style={{
                            background: isSelected ? withAlpha("var(--accent-primary)", 10) : "transparent",
                            borderLeft: isSelected ? "2px solid var(--accent-primary)" : "2px solid transparent",
                          }}
                        >
                          <span className="flex items-center gap-2 w-full">
                            {isSelected && (
                              <span
                                className="w-1.5 h-1.5 rounded-full"
                                style={{ background: "var(--accent-primary)" }}
                              />
                            )}
                            <span>
                              v{version}
                              {timestamp && (
                                <span style={{ color: "var(--text-muted)" }}> — {timestamp}</span>
                              )}
                            </span>
                            {isLatest && (
                              <span className="ml-auto" style={{ color: "var(--text-muted)" }}>
                                (latest)
                              </span>
                            )}
                          </span>
                        </DropdownMenuItem>
                      );
                    })}
                  </DropdownMenuContent>
                </DropdownMenu>
              )}

              {showOverflowActions && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 w-7 p-0 rounded-lg transition-colors duration-150"
                      style={{ color: "var(--text-muted)" }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.background = "var(--overlay-weak)";
                        e.currentTarget.style.color = "var(--text-primary)";
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.background = "transparent";
                        e.currentTarget.style.color = "var(--text-muted)";
                      }}
                      aria-label={`${artifactLabel} actions`}
                    >
                      <MoreHorizontal className="w-3.5 h-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="w-44"
                    style={{
                      background: "var(--bg-elevated)",
                      backdropFilter: "blur(20px)",
                      border: "1px solid var(--overlay-weak)",
                      boxShadow: "var(--shadow-lg)",
                    }}
                  >
                    <DropdownMenuItem
                      onClick={() => {
                        const content = plan.content.type === "inline" ? plan.content.text : "";
                        navigator.clipboard.writeText(content).then(() => {
                          toast.success("Copied to clipboard");
                        }).catch(() => {
                          toast.error("Failed to copy");
                        });
                      }}
                      disabled={plan.content.type !== "inline" || !plan.content.text}
                      className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                    >
                      <Copy className="w-3.5 h-3.5" />
                      Copy Markdown
                    </DropdownMenuItem>
                    {onStartNewConversationWithPlan &&
                      (!disableHistoricalNewConversation || !isViewingHistorical) && (
                      <DropdownMenuItem
                        onClick={handleStartNewConversationWithPlan}
                        className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                      >
                        <MessageSquarePlus className="w-3.5 h-3.5" />
                        New Conversation
                      </DropdownMenuItem>
                    )}
                    {onEdit && (
                      <DropdownMenuItem
                        onClick={onEdit}
                        className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                      >
                        <FileEdit className="w-3.5 h-3.5" />
                        Edit
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem
                      onClick={handleExport}
                      className="text-[0.75rem] cursor-pointer gap-2 px-3 py-2"
                    >
                      <Download className="w-3.5 h-3.5" />
                      Export...
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>
          </div>
        </div>

        {/* Content */}
        <CollapsibleContent>
          <div
            className="mt-3 pl-6 pr-2 pb-4"
            style={{
              marginLeft: "16px",
              borderLeft: "2px solid var(--accent-border)",
            }}
          >
            {/* Version banner when viewing historical */}
            {isViewingHistorical && (
              <div
                className="flex items-center justify-between mb-4 px-3 py-2.5 rounded-lg"
                style={{
                  background: "var(--status-warning-muted)",
                  border: "1px solid var(--status-warning-border)",
                }}
              >
                <span className="text-[0.75rem] font-medium" style={{ color: "var(--status-warning)" }}>
                  Viewing version {selectedVersion} of {plan.metadata.version}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleBackToLatest}
                  className="h-6 px-2.5 text-[0.6875rem] font-medium gap-1.5 rounded-md transition-colors duration-150"
                  style={{
                    color: "var(--status-warning)",
                    background: "var(--status-warning-muted)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = withAlpha("var(--status-warning)", 20);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "var(--status-warning-muted)";
                  }}
                >
                  <ArrowLeft className="w-3 h-3" />
                  Back to latest
                </Button>
              </div>
            )}

            {!isLoadingVersion && artifactPreamble}

            {/* Loading state / plan content */}
            {isLoadingVersion ? (
                <div className="flex items-center justify-center py-12">
                  <div className="flex flex-col items-center gap-3">
                    <Loader2 className="w-6 h-6 animate-spin" style={{ color: "var(--accent-primary)" }} />
                    <span className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
                      Loading version...
                    </span>
                  </div>
                </div>
              ) : renderedContent ? (
                <ArtifactMarkdownBody
                  content={renderedContent}
                  source={{
                    sourceKind: artifactLabel === "Review" ? "review" : "plan",
                    sourceId: plan.id,
                    sourceLabel: artifactLabel,
                    title: plan.name,
                    artifactId: plan.id,
                    version: selectedVersion,
                  }}
                  excerptSelectionEnabled={excerptSelectionEnabled}
                  className="text-[0.8125rem] leading-relaxed"
                />
              ) : (
                <p
                  className="text-[0.8125rem] italic py-8 text-center"
                  style={{ color: "var(--text-muted)" }}
                >
                  No content available
                </p>
              )}
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}

export interface VersionedArtifactDisplayProps
  extends Omit<PlanDisplayProps, "plan"> {
  artifact: Artifact;
}

/** Shared versioned Markdown artifact surface used by Plan, Review, and Persona. */
export function VersionedArtifactDisplay({
  artifact,
  ...props
}: VersionedArtifactDisplayProps) {
  return <PlanDisplay plan={artifact} {...props} />;
}
