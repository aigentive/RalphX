/**
 * CandidateCard - renders one card per ProjectCandidate probe verdict with
 * per-verdict recovery actions. Copy is plain-language only: no git commands,
 * ref syntax, or error codes on screen.
 */

import { AlertTriangle, CheckCircle2, FolderInput, GitPullRequest, Info } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ProjectCandidate } from "@/types/project-candidate";

export interface CandidateCardProps {
  candidate: ProjectCandidate;
  onSwitchToCreateNew: (path: string) => void;
  onUseRepositoryRoot: (repositoryRoot: string) => void;
  onOpenExisting: () => void;
  probedPath: string;
}

function CardShell({
  tone,
  icon,
  title,
  description,
  children,
}: {
  tone: "neutral" | "warning" | "error" | "success";
  icon: React.ReactNode;
  title: string;
  description: string;
  children?: React.ReactNode;
}) {
  const toneColor =
    tone === "error"
      ? "var(--status-error)"
      : tone === "warning"
        ? "var(--status-warning)"
        : tone === "success"
          ? "var(--accent-primary)"
          : "var(--text-muted)";
  return (
    <div
      data-testid="candidate-card"
      className="flex gap-3 p-3 rounded-lg bg-[var(--bg-base)]"
      style={{ border: "1px solid var(--border-subtle)" }}
    >
      <span className="mt-0.5 flex-shrink-0" style={{ color: toneColor }}>
        {icon}
      </span>
      <div className="flex-1 min-w-0 space-y-2">
        <div>
          <div className="text-sm font-medium text-[var(--text-primary)]">{title}</div>
          <div className="text-xs mt-0.5 text-[var(--text-muted)]">{description}</div>
        </div>
        {children}
      </div>
    </div>
  );
}

function RecoveryButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      data-testid="candidate-recovery-action"
      onClick={onClick}
      className={cn(
        "text-xs font-medium px-2.5 py-1.5 rounded-md transition-colors",
        "bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
      )}
    >
      {label}
    </button>
  );
}

export function CandidateCard({
  candidate,
  onSwitchToCreateNew,
  onUseRepositoryRoot,
  onOpenExisting,
  probedPath,
}: CandidateCardProps) {
  switch (candidate.kind) {
    case "notFound":
      return (
        <CardShell
          tone="error"
          icon={<AlertTriangle className="h-4 w-4" />}
          title="Folder not found"
          description="This folder doesn't exist yet. Choose a different folder, or use Create New Repository instead."
        />
      );
    case "notADirectory":
      return (
        <CardShell
          tone="error"
          icon={<AlertTriangle className="h-4 w-4" />}
          title="Not a folder"
          description="The selected path isn't a folder. Choose a different folder."
        />
      );
    case "emptyDirectory":
      return (
        <CardShell
          tone="neutral"
          icon={<Info className="h-4 w-4" />}
          title="Empty folder"
          description="This folder is empty. RalphX will set it up as a new repository when you continue."
        />
      );
    case "nonEmptyNonRepo":
      return (
        <CardShell
          tone="warning"
          icon={<AlertTriangle className="h-4 w-4" />}
          title="Not a repository yet"
          description={`This folder already has ${candidate.entryCount} item${candidate.entryCount === 1 ? "" : "s"} but isn't set up with version control.`}
        >
          <RecoveryButton
            label="Use Create New Repository instead"
            onClick={() => onSwitchToCreateNew(probedPath)}
          />
        </CardShell>
      );
    case "nestedInRepository":
      return (
        <CardShell
          tone="warning"
          icon={<FolderInput className="h-4 w-4" />}
          title="Inside another repository"
          description={`This folder is inside a repository rooted at ${candidate.repositoryRoot}.`}
        >
          <RecoveryButton
            label="Use the repository root instead"
            onClick={() => onUseRepositoryRoot(candidate.repositoryRoot)}
          />
        </CardShell>
      );
    case "detachedHead":
      return (
        <CardShell
          tone="error"
          icon={<AlertTriangle className="h-4 w-4" />}
          title="No active branch"
          description="This repository isn't currently on a branch. Switch to a branch in this folder before adding it here."
        />
      );
    case "inspectionFailed":
      return (
        <CardShell
          tone="neutral"
          icon={<Info className="h-4 w-4" />}
          title="Couldn't inspect this folder"
          description="We couldn't check this folder in detail, but you can still continue."
        />
      );
    case "repository": {
      if (candidate.alreadyRegisteredAs) {
        return (
          <CardShell
            tone="error"
            icon={<AlertTriangle className="h-4 w-4" />}
            title="Already added to RalphX"
            description={`"${candidate.alreadyRegisteredAs.name}" already uses this folder.`}
          >
            <RecoveryButton
              label={`Open ${candidate.alreadyRegisteredAs.name} instead`}
              onClick={onOpenExisting}
            />
          </CardShell>
        );
      }
      return (
        <CardShell
          tone="success"
          icon={<CheckCircle2 className="h-4 w-4" />}
          title="Repository found"
          description={`Currently on "${candidate.currentBranch}"${candidate.isDirty ? " with uncommitted changes" : ""}.`}
        >
          {candidate.capability.kind === "github" && (
            <span
              className="inline-flex items-center gap-1.5 text-xs px-2 py-1 rounded-md"
              style={{ backgroundColor: "var(--bg-elevated)", color: "var(--text-secondary)" }}
            >
              <GitPullRequest className="h-3.5 w-3.5" />
              GitHub pull requests available
            </span>
          )}
        </CardShell>
      );
    }
  }
}
