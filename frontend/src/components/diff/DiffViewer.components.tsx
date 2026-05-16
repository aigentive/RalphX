/**
 * DiffViewer sub-components
 *
 * Extracted from DiffViewer.tsx to reduce file size.
 */

import { useState, useMemo, useCallback } from "react";
import {
  Folder,
  FolderOpen,
  ChevronRight,
  ExternalLink,
  GitCommit,
  CheckCircle2,
  FileSearch,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from "@/components/ui/tooltip";
import { Skeleton } from "@/components/ui/skeleton";
import { SimpleDiffView } from "./SimpleDiffView";
import type { DiffRefKind, PrDiffAnnotation } from "@/api/diff";

import {
  type FileChange,
  type Commit,
  type DiffData,
  type TreeNode,
  getStatusColor,
  getStatusLetter,
  formatRelativeDate,
  getFileIcon,
  buildFileTree,
} from "./DiffViewer.types";

// ============================================================================
// FileTreeItem
// ============================================================================

interface FileTreeItemProps {
  node: TreeNode;
  depth: number;
  selectedPath: string | null;
  onSelect: (path: string) => void;
  onToggleExpand?: ((path: string) => void) | undefined;
  expandedDirs: Set<string>;
}

export function FileTreeItem({
  node,
  depth,
  selectedPath,
  onSelect,
  onToggleExpand,
  expandedDirs,
}: FileTreeItemProps) {
  const isSelected = selectedPath === node.path;
  const isExpanded = node.isDirectory && expandedDirs.has(node.path);

  if (node.isDirectory) {
    return (
      <>
        <button
          data-testid={`dir-${node.path}`}
          onClick={() => onToggleExpand?.(node.path)}
          className={cn(
            "w-full flex items-center gap-1 px-2 py-1 text-sm transition-colors",
            "hover:bg-[var(--bg-hover)] cursor-pointer"
          )}
          style={{
            paddingLeft: `${depth * 16 + 8}px`,
            height: "28px",
          }}
        >
          <span
            className="transition-transform duration-150"
            style={{
              transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)",
            }}
          >
            <ChevronRight className="w-4 h-4 text-[var(--text-muted)]" />
          </span>
          {isExpanded ? (
            <FolderOpen className="w-4 h-4 text-[var(--text-muted)]" />
          ) : (
            <Folder className="w-4 h-4 text-[var(--text-muted)]" />
          )}
          <span className="truncate text-[var(--text-secondary)]">{node.name}</span>
        </button>
        {isExpanded && (
          <div>
            {node.children.map((child) => (
              <FileTreeItem
                key={child.path}
                node={child}
                depth={depth + 1}
                selectedPath={selectedPath}
                onSelect={onSelect}
                onToggleExpand={onToggleExpand}
                expandedDirs={expandedDirs}
              />
            ))}
          </div>
        )}
      </>
    );
  }

  const file = node.file;
  const statusColor = getStatusColor(file.status);

  return (
    <button
      data-testid={`file-${file.path}`}
      onClick={() => onSelect(file.path)}
      className={cn(
        "w-full flex items-center gap-1 px-2 py-1 text-sm transition-colors",
        "hover:bg-[var(--bg-hover)] cursor-pointer",
        isSelected && "bg-[var(--bg-elevated)]"
      )}
      style={{
        paddingLeft: `${depth * 16 + 8}px`,
        height: "28px",
      }}
    >
      {/* Spacer for chevron alignment */}
      <span className="w-4" />
      <span className="text-[var(--text-muted)]">
        {getFileIcon(file.path)}
      </span>
      <span
        className={cn(
          "truncate flex-1 text-left",
          isSelected ? "text-[var(--text-primary)]" : "text-[var(--text-secondary)]"
        )}
      >
        {node.name}
      </span>
      <span
        className="text-xs font-mono w-4 text-center"
        style={{ color: statusColor }}
      >
        {getStatusLetter(file.status)}
      </span>
    </button>
  );
}

// ============================================================================
// FileTree
// ============================================================================

interface FileTreeProps {
  files: FileChange[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  emptyTitle?: string;
  emptySubtitle?: string;
}

export function FileTree({
  files,
  selectedPath,
  onSelect,
  emptyTitle = "No uncommitted changes",
  emptySubtitle = "Your working directory is clean",
}: FileTreeProps) {
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(() => {
    // Initially expand all directories
    const dirs = new Set<string>();
    for (const file of files) {
      const parts = file.path.split("/");
      let path = "";
      for (let i = 0; i < parts.length - 1; i++) {
        const part = parts[i];
        if (part === undefined) continue;
        path = path ? `${path}/${part}` : part;
        dirs.add(path);
      }
    }
    return dirs;
  });

  const tree = useMemo(() => buildFileTree(files), [files]);

  const handleToggleExpand = useCallback((path: string) => {
    setExpandedDirs((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  if (files.length === 0) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full p-16 text-center"
        data-testid="file-tree-empty"
      >
        <CheckCircle2 className="w-12 h-12 text-[var(--text-muted)] opacity-50 mb-4" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">{emptyTitle}</p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          {emptySubtitle}
        </p>
      </div>
    );
  }

  return (
    <div className="py-2" data-testid="file-tree">
      {tree.map((node) => (
        <FileTreeItem
          key={node.path}
          node={node}
          depth={0}
          selectedPath={selectedPath}
          onSelect={onSelect}
          onToggleExpand={handleToggleExpand}
          expandedDirs={expandedDirs}
        />
      ))}
    </div>
  );
}

// ============================================================================
// CommitList
// ============================================================================

interface CommitListProps {
  commits: Commit[];
  selectedSha: string | null;
  onSelect: (commit: Commit) => void;
}

export function CommitList({ commits, selectedSha, onSelect }: CommitListProps) {
  if (commits.length === 0) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full p-16 text-center"
        data-testid="commit-list-empty"
      >
        <GitCommit className="w-12 h-12 text-[var(--text-muted)] opacity-50 mb-4" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">No commit history</p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          Make your first commit to see history here
        </p>
      </div>
    );
  }

  return (
    <div className="py-2" data-testid="commit-list">
      {commits.map((commit) => {
        const isSelected = selectedSha === commit.sha;
        return (
          <button
            key={commit.sha}
            data-testid={`commit-${commit.shortSha}`}
            onClick={() => onSelect(commit)}
            className={cn(
              "w-full flex items-center px-3 py-2 text-left transition-colors cursor-pointer",
              "border-b border-[var(--border-subtle)]",
              "hover:bg-[var(--bg-hover)]",
              isSelected && "bg-[var(--bg-elevated)] border-l-2 border-l-[var(--accent-primary)]"
            )}
            style={{ height: "48px" }}
          >
            <span className="text-xs font-mono text-[var(--accent-primary)] mr-2 shrink-0">
              {commit.shortSha}
            </span>
            <span className="text-sm text-[var(--text-primary)] truncate flex-1">
              {commit.message}
            </span>
            <span className="text-xs text-[var(--text-muted)] ml-2 whitespace-nowrap shrink-0">
              {commit.author} • {formatRelativeDate(commit.date)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

// ============================================================================
// DiffPanel
// ============================================================================

interface DiffPanelProps {
  diffData: DiffData | null;
  isLoading: boolean;
  filePath: string | null;
  onOpenInIDE?: ((path: string) => void) | undefined;
  conversationId?: string;
  refKind?: DiffRefKind;
  annotations?: PrDiffAnnotation[];
}

export function DiffPanel({
  diffData,
  isLoading,
  filePath,
  onOpenInIDE,
  conversationId,
  refKind,
  annotations = [],
}: DiffPanelProps) {
  if (isLoading) {
    return (
      <div
        className="flex items-center justify-center h-full"
        data-testid="diff-loading"
      >
        <div className="flex flex-col items-center gap-3">
          <Loader2 className="w-6 h-6 animate-spin text-[var(--accent-primary)]" />
          <p className="text-sm text-[var(--text-secondary)]">Loading diff...</p>
        </div>
      </div>
    );
  }

  if (!filePath) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full p-16 text-center"
        data-testid="diff-empty"
      >
        <FileSearch className="w-12 h-12 text-[var(--text-muted)] opacity-50 mb-4" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">
          Select a file to view changes
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          Click on a file in the tree to see its diff
        </p>
      </div>
    );
  }

  if (!diffData) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full p-16 text-center"
        data-testid="diff-error"
      >
        <p className="text-sm text-[var(--text-secondary)]">
          Unable to load diff for this file
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full" data-testid="diff-content">
      {/* File header */}
      <div
        className="flex items-center justify-between px-4 shrink-0"
        style={{
          height: "40px",
          borderBottom: "1px solid var(--border-subtle)",
          backgroundColor: "var(--bg-surface)",
        }}
      >
        <span
          className="font-mono text-sm truncate"
          style={{ color: "var(--text-secondary)" }}
        >
          {filePath}
        </span>
        {onOpenInIDE && (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                  onClick={() => onOpenInIDE(filePath)}
                  data-testid="open-in-ide"
                >
                  <ExternalLink className="w-4 h-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Open in IDE</p>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}
      </div>

      {/* Diff view - using SimpleDiffView for reliable rendering */}
      <div className="flex-1 overflow-hidden">
        <SimpleDiffView
          hunks={diffData.hunks}
          oldTotalLines={diffData.oldTotalLines}
          newTotalLines={diffData.newTotalLines}
          isBinary={diffData.isBinary}
          language={diffData.language}
          {...(conversationId !== undefined && refKind !== undefined && {
            conversationId,
            filePath,
            refKind,
          })}
          annotations={annotations}
        />
      </div>
    </div>
  );
}

// ============================================================================
// CommitDiffPanel
// ============================================================================

interface CommitDiffPanelProps {
  commit: Commit | null;
  files: FileChange[];
  selectedFilePath: string | null;
  onSelectFile: (path: string) => void;
  diffData: DiffData | null;
  isLoading: boolean;
  isLoadingFiles?: boolean;
  emptyTitle?: string;
  emptySubtitle?: string;
  onOpenInIDE?: ((path: string) => void) | undefined;
  conversationId?: string;
  refKind?: DiffRefKind;
}

export function CommitDiffPanel({
  commit,
  files,
  selectedFilePath,
  onSelectFile,
  diffData,
  isLoading,
  isLoadingFiles = false,
  emptyTitle = "No files changed",
  emptySubtitle = "This commit did not report file changes",
  onOpenInIDE,
  conversationId,
  refKind,
}: CommitDiffPanelProps) {
  if (!commit) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full p-16 text-center"
        data-testid="commit-diff-empty"
      >
        <GitCommit className="w-12 h-12 text-[var(--text-muted)] opacity-50 mb-4" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">
          Select a commit to view changes
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          Changed files will be displayed here
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {/* Files changed in this commit */}
      <div
        className="w-64 shrink-0 overflow-hidden border-r border-[var(--border-subtle)]"
        style={{ minWidth: "200px" }}
      >
        <div
          className="px-3 py-2 border-b border-[var(--border-subtle)]"
          style={{ backgroundColor: "var(--bg-surface)" }}
        >
          <p className="text-xs font-medium text-[var(--text-muted)]">
            {files.length} file{files.length !== 1 ? "s" : ""} changed
          </p>
        </div>
        <ScrollArea className="h-[calc(100%-33px)]">
          {isLoadingFiles ? (
            <FileTreeSkeleton />
          ) : (
            <FileTree
              files={files}
              selectedPath={selectedFilePath}
              onSelect={onSelectFile}
              emptyTitle={emptyTitle}
              emptySubtitle={emptySubtitle}
            />
          )}
        </ScrollArea>
      </div>

      {/* Diff */}
      <div className="flex-1 min-w-0">
        <DiffPanel
          diffData={diffData}
          isLoading={isLoading}
          filePath={selectedFilePath}
          {...(onOpenInIDE !== undefined && { onOpenInIDE })}
          {...(conversationId !== undefined && { conversationId })}
          {...(refKind !== undefined && { refKind })}
        />
      </div>
    </div>
  );
}

// ============================================================================
// Skeleton
// ============================================================================

// Predefined widths to avoid Math.random() in render
const SKELETON_WIDTHS = ["75%", "85%", "65%", "90%", "70%", "80%", "60%"];

export function FileTreeSkeleton() {
  return (
    <div className="py-2 px-2 space-y-1">
      {SKELETON_WIDTHS.map((width, i) => (
        <Skeleton key={i} className="h-7" style={{ width }} />
      ))}
    </div>
  );
}
