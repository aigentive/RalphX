import {
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AlertCircle,
  CheckCircle2,
  Clipboard,
  Download,
  Eye,
  FilePenLine,
  Loader2,
  Send,
} from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";

import { agentIssueReportApi } from "@/api/agent-issue-report";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import type { AgentIssueReportDraft } from "@/api/agent-issue-report";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";

interface AgentIssueReportContext {
  projectId: string;
  conversationId: string;
}

interface AgentIssueReportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  context: AgentIssueReportContext | null;
}

const LazyMarkdown = lazyWithRetry(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] =
    await Promise.all([import("react-markdown"), import("remark-gfm")]);

  return {
    default: memo(function IssueReportMarkdown({ body }: { body: string }) {
      return (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={markdownComponents}
          skipHtml
        >
          {body}
        </ReactMarkdown>
      );
    }),
  };
});

type ReportMode = "edit" | "preview";
type SubmitPhase = "idle" | "confirm" | "submitting" | "submitted";

function defaultIssueTitle(conversationId: string): string {
  return `RalphX issue report: ${conversationId.slice(0, 8)}`;
}

function destinationLabel(draft: AgentIssueReportDraft): string {
  return draft.destination.source === "configured"
    ? "Configured destination"
    : "Public default destination";
}

export function AgentIssueReportDialog({
  open,
  onOpenChange,
  context,
}: AgentIssueReportDialogProps) {
  const [draft, setDraft] = useState<AgentIssueReportDraft | null>(null);
  const [body, setBody] = useState("");
  const [issueTitle, setIssueTitle] = useState("");
  const [mode, setMode] = useState<ReportMode>("edit");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitPhase, setSubmitPhase] = useState<SubmitPhase>("idle");
  const [issueUrl, setIssueUrl] = useState<string | null>(null);
  const [isExporting, setIsExporting] = useState(false);
  const loadRequestRef = useRef(0);
  const isRemoteEnvironment = useIsRemoteEnvironment();

  const reportConversationId = context?.conversationId ?? null;
  const reportProjectId = context?.projectId ?? null;

  useEffect(() => {
    if (!open || !context || isRemoteEnvironment) {
      setDraft(null);
      setBody("");
      setIssueTitle("");
      setMode("edit");
      setIsLoading(false);
      setError(null);
      setSubmitPhase("idle");
      setIssueUrl(null);
      return;
    }

    const requestId = loadRequestRef.current + 1;
    loadRequestRef.current = requestId;
    setDraft(null);
    setBody("");
    setIssueTitle(defaultIssueTitle(context.conversationId));
    setMode("edit");
    setIsLoading(true);
    setError(null);
    setSubmitPhase("idle");
    setIssueUrl(null);

    const startLoad = () => {
      void agentIssueReportApi
        .build({
          conversationId: context.conversationId,
          projectId: context.projectId,
          includeLogs: true,
        })
        .then((nextDraft) => {
          if (loadRequestRef.current !== requestId) return;
          setDraft(nextDraft);
          setBody(nextDraft.markdown);
          setIsLoading(false);
        })
        .catch((unknownError) => {
          if (loadRequestRef.current !== requestId) return;
          const message =
            unknownError instanceof Error
              ? unknownError.message
              : "Failed to build issue report";
          setError(message);
          setIsLoading(false);
        });
    };

    if (typeof window === "undefined") {
      startLoad();
      return;
    }
    const frame = window.requestAnimationFrame(startLoad);
    return () => window.cancelAnimationFrame(frame);
  }, [open, context, isRemoteEnvironment]);

  const redactionText = useMemo(() => {
    if (!draft || draft.redactionSummary.replacements.length === 0) {
      return "No automated redactions";
    }
    return draft.redactionSummary.replacements
      .map((entry) => `${entry.category}: ${entry.count}`)
      .join(", ");
  }, [draft]);

  const handleCopy = useCallback(async () => {
    if (!body) return;
    try {
      await navigator.clipboard.writeText(body);
      toast.success("Issue report copied");
    } catch {
      toast.error("Failed to copy issue report");
    }
  }, [body]);

  const handleExport = useCallback(async () => {
    if (!body || !reportConversationId) return;
    setIsExporting(true);
    try {
      const savePath = await save({
        filters: [{ name: "Markdown", extensions: ["md"] }],
        defaultPath: `ralphx-issue-report-${reportConversationId.slice(0, 8)}.md`,
      });
      if (savePath === null) return;
      await writeTextFile(savePath, body);
      toast.success("Issue report exported");
    } catch {
      toast.error("Failed to export issue report");
    } finally {
      setIsExporting(false);
    }
  }, [body, reportConversationId]);

  const handleSubmit = useCallback(async () => {
    if (!draft || !reportConversationId || !body.trim()) return;
    if (submitPhase !== "confirm") {
      setSubmitPhase("confirm");
      return;
    }

    setSubmitPhase("submitting");
    try {
      const response = await agentIssueReportApi.submit({
        conversationId: reportConversationId,
        repository: draft.destination.repository,
        title: issueTitle,
        bodyMarkdown: body,
      });
      setIssueUrl(response.issueUrl);
      setSubmitPhase("submitted");
      toast.success("GitHub issue created");
    } catch (unknownError) {
      const message =
        unknownError instanceof Error
          ? unknownError.message
          : "Failed to create GitHub issue";
      setError(message);
      setSubmitPhase("idle");
    }
  }, [body, draft, issueTitle, reportConversationId, submitPhase]);

  const canSubmit = Boolean(draft && body.trim() && issueTitle.trim());

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[88vh] max-w-[980px] grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <DialogHeader>
          <div className="min-w-0">
            <DialogTitle>Report Issue</DialogTitle>
            <DialogDescription>
              Review the generated context before creating the GitHub issue.
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="min-h-0 overflow-hidden px-6 py-4">
          {isRemoteEnvironment && (
            <RemoteHostOnlyNotice subject="Issue reports" />
          )}
          {!isRemoteEnvironment && !context && (
            <div
              className="flex h-[280px] items-center justify-center rounded-md border text-sm"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-secondary)",
              }}
            >
              Select an agent conversation to report an issue.
            </div>
          )}

          {!isRemoteEnvironment && context && (
            <div className="flex h-full min-h-0 flex-col gap-3">
              <div
                className="grid gap-2 rounded-md border px-3 py-2 text-xs sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                <div className="min-w-0">
                  <div style={{ color: "var(--text-muted)" }}>Conversation</div>
                  <div className="truncate font-mono" style={{ color: "var(--text-primary)" }}>
                    {reportConversationId}
                  </div>
                </div>
                <div className="min-w-0">
                  <div style={{ color: "var(--text-muted)" }}>Project</div>
                  <div className="truncate font-mono" style={{ color: "var(--text-primary)" }}>
                    {reportProjectId}
                  </div>
                </div>
                <div className="min-w-0 sm:text-right">
                  <div style={{ color: "var(--text-muted)" }}>Destination</div>
                  <div className="truncate font-mono" style={{ color: "var(--text-primary)" }}>
                    {draft ? draft.destination.repository : "Loading"}
                  </div>
                </div>
              </div>

              {isLoading && (
                <div
                  className="flex h-[360px] items-center justify-center gap-2 rounded-md border text-sm"
                  data-testid="agent-issue-report-loading"
                  style={{
                    backgroundColor: "var(--bg-surface)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    color: "var(--text-secondary)",
                  }}
                >
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Building report
                </div>
              )}

              {error && !isLoading && (
                <div
                  className="flex items-start gap-2 rounded-md border px-3 py-2 text-sm"
                  role="alert"
                  style={{
                    backgroundColor: "var(--bg-danger-subtle, var(--bg-surface))",
                    borderColor: "var(--border-danger, var(--border-subtle))",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    color: "var(--text-primary)",
                  }}
                >
                  <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                  <span>{error}</span>
                </div>
              )}

              {draft && !isLoading && (
                <>
                  <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_220px]">
                    <label className="min-w-0 text-xs font-medium" style={{ color: "var(--text-secondary)" }}>
                      Issue title
                      <input
                        value={issueTitle}
                        onChange={(event) => setIssueTitle(event.target.value)}
                        className="mt-1 h-9 w-full rounded-md border px-3 text-sm outline-none focus-visible:[outline:2px_solid_var(--border-focus)]"
                        style={{
                          backgroundColor: "var(--bg-surface)",
                          borderColor: "var(--border-subtle)",
                          borderStyle: "solid",
                          borderWidth: "1px",
                          color: "var(--text-primary)",
                        }}
                      />
                    </label>
                    <div
                      className="rounded-md border px-3 py-2 text-xs"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                      }}
                    >
                      <div style={{ color: "var(--text-muted)" }}>
                        {destinationLabel(draft)}
                      </div>
                      <div className="truncate font-mono" style={{ color: "var(--text-primary)" }}>
                        {draft.destination.repository}
                      </div>
                      <div className="mt-1 truncate" style={{ color: "var(--text-muted)" }}>
                        {redactionText}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center justify-between gap-3">
                    <div
                      className="inline-flex rounded-md border p-0.5"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                      }}
                    >
                      <button
                        type="button"
                        onClick={() => setMode("edit")}
                        className={cn(
                          "inline-flex h-8 items-center gap-1.5 rounded px-3 text-xs font-medium",
                          mode === "edit"
                            ? "bg-[var(--bg-hover)] text-[var(--text-primary)]"
                            : "text-[var(--text-secondary)]"
                        )}
                        data-testid="agent-issue-report-edit-tab"
                      >
                        <FilePenLine className="h-3.5 w-3.5" />
                        Edit
                      </button>
                      <button
                        type="button"
                        onClick={() => setMode("preview")}
                        className={cn(
                          "inline-flex h-8 items-center gap-1.5 rounded px-3 text-xs font-medium",
                          mode === "preview"
                            ? "bg-[var(--bg-hover)] text-[var(--text-primary)]"
                            : "text-[var(--text-secondary)]"
                        )}
                        data-testid="agent-issue-report-preview-tab"
                      >
                        <Eye className="h-3.5 w-3.5" />
                        Preview
                      </button>
                    </div>
                    <div className="hidden min-w-0 truncate text-xs sm:block" style={{ color: "var(--text-muted)" }}>
                      Sources: {draft.sources.filter((source) => source.included).length}
                      {draft.warnings.length > 0 ? ` | Warnings: ${draft.warnings.length}` : ""}
                    </div>
                  </div>

                  <div
                    className="min-h-0 flex-1 overflow-hidden rounded-md border"
                    style={{
                      backgroundColor: "var(--bg-surface)",
                      borderColor: "var(--border-subtle)",
                      borderStyle: "solid",
                      borderWidth: "1px",
                    }}
                  >
                    {mode === "edit" ? (
                      <Textarea
                        value={body}
                        onChange={(event) => {
                          setBody(event.target.value);
                          if (submitPhase === "confirm") setSubmitPhase("idle");
                        }}
                        className="h-full min-h-[320px] resize-none rounded-none border-0 font-mono text-xs shadow-none focus-visible:ring-0"
                        data-testid="agent-issue-report-editor"
                        style={{
                          backgroundColor: "var(--bg-surface)",
                          color: "var(--text-primary)",
                        }}
                      />
                    ) : (
                      <div
                        className="h-full min-h-[320px] overflow-auto px-4 py-3 text-sm"
                        data-testid="agent-issue-report-preview"
                      >
                        <Suspense
                          fallback={
                            <div className="flex items-center gap-2 text-sm" style={{ color: "var(--text-secondary)" }}>
                              <Loader2 className="h-4 w-4 animate-spin" />
                              Rendering preview
                            </div>
                          }
                        >
                          <LazyMarkdown body={body} />
                        </Suspense>
                      </div>
                    )}
                  </div>

                  {submitPhase === "confirm" && (
                    <div
                      className="rounded-md border px-3 py-2 text-sm"
                      data-testid="agent-issue-report-confirm"
                      style={{
                        backgroundColor: "var(--bg-warning-subtle, var(--bg-surface))",
                        borderColor: "var(--border-warning, var(--border-subtle))",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-primary)",
                      }}
                    >
                      This will create a GitHub issue in{" "}
                      <span className="font-mono">{draft.destination.repository}</span> using
                      the edited Markdown above.
                    </div>
                  )}

                  {submitPhase === "submitted" && issueUrl && (
                    <div
                      className="flex items-center gap-2 rounded-md border px-3 py-2 text-sm"
                      style={{
                        backgroundColor: "var(--bg-success-subtle, var(--bg-surface))",
                        borderColor: "var(--border-success, var(--border-subtle))",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-primary)",
                      }}
                    >
                      <CheckCircle2 className="h-4 w-4" />
                      <a href={issueUrl} target="_blank" rel="noreferrer" className="truncate underline">
                        {issueUrl}
                      </a>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </div>

        {!isRemoteEnvironment && <DialogFooter>
          <Button variant="outline" onClick={handleCopy} disabled={!body}>
            <Clipboard className="h-4 w-4" />
            Copy
          </Button>
          <Button
            variant="outline"
            onClick={handleExport}
            disabled={!body || isExporting}
          >
            {isExporting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            Export
          </Button>
          {submitPhase === "confirm" && (
            <Button variant="outline" onClick={() => setSubmitPhase("idle")}>
              Cancel Submit
            </Button>
          )}
          <Button
            onClick={handleSubmit}
            disabled={!canSubmit || submitPhase === "submitting" || submitPhase === "submitted"}
            data-testid="agent-issue-report-submit"
          >
            {submitPhase === "submitting" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : submitPhase === "confirm" ? (
              <CheckCircle2 className="h-4 w-4" />
            ) : (
              <Send className="h-4 w-4" />
            )}
            {submitPhase === "confirm" ? "Confirm and Create" : "Create GitHub Issue"}
          </Button>
        </DialogFooter>}
      </DialogContent>
    </Dialog>
  );
}
