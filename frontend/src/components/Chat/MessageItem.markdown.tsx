/* eslint-disable react-refresh/only-export-components */
/**
 * MessageItem.markdown - Markdown rendering components for MessageItem
 *
 * Contains:
 * - CodeBlock: Code display with copy functionality
 * - markdownComponents: ReactMarkdown component overrides
 */

import React, { useState, useCallback } from "react";
import {
  Check,
  ChevronDown,
  Code2,
  Copy,
  FolderOpen,
  Loader2,
  Terminal as TerminalIcon,
} from "lucide-react";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

import { HostPathCopyButton } from "@/components/remote/HostPathCopyButton";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { logger } from "@/lib/logger";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  resolvePreferredWorkspaceOpenTarget,
} from "@/lib/workspace-open-targets";
import { useMessageFileLinkContext } from "./MessageFileLinkContext";

// Percentage max-widths resolve against the nearest block container. Inside the
// shrink-to-fit user bubble (w-fit) that container is the bubble itself, which
// collapses short messages into mid-word wraps — so the bubble overrides this
// cap via --chat-prose-max-width (see TextBubble).
const PROSE_MAX_WIDTH = "var(--chat-prose-max-width, min(85%, 620px))";

const BOX_DRAWING_RE = /[┌┐└┘├┤┬┴┼─│╔╗╚╝╠╣╦╩╬═║░▓█▒╮╰╯╭]/g;
const ASCII_DRAWING_SEQUENCE_RE = /[+\-|=]{3,}/;
const ASCII_DRAWING_LINE_RE = /^[\s+\-|=]+$/;

function looksLikeAsciiArt(children: React.ReactNode): boolean {
  const text = extractText(children);
  if (!text) return false;
  if ((text.match(BOX_DRAWING_RE)?.length ?? 0) >= 2) {
    return true;
  }

  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .some(
      (line) =>
        line.length >= 3 &&
        ASCII_DRAWING_SEQUENCE_RE.test(line) &&
        ASCII_DRAWING_LINE_RE.test(line)
    );
}

function extractText(node: React.ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  if (React.isValidElement<{ children?: React.ReactNode }>(node)) {
    if (node.type === "code") {
      return "";
    }
    return extractText(node.props.children);
  }
  return "";
}

// ============================================================================
// Code Block with Copy Button
// ============================================================================

interface CodeBlockProps {
  children: string;
  language?: string | undefined;
}

export function CodeBlock({ children, language }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(children);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Silently fail
    }
  }, [children]);

  return (
    <div className="relative group my-2 max-w-full overflow-hidden">
      {language && (
        <span
          className="absolute top-1.5 left-3 text-[0.625rem] uppercase tracking-wide"
          style={{ color: "var(--text-muted)" }}
        >
          {language}
        </span>
      )}
      <pre
        className="rounded-lg overflow-x-auto max-w-full"
        style={{
          /* macOS Tahoe: flat dark background, no border */
          backgroundColor: "var(--bg-surface)",
          border: "none",
        }}
      >
        <code
          className={cn("block p-3 text-[0.75rem]", language && "pt-7")}
          style={{
            fontFamily: "var(--font-mono)",
            color: "var(--text-primary)",
            whiteSpace: "pre",
          }}
        >
          {children}
        </code>
      </pre>
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={handleCopy}
        className="absolute top-1 right-1 opacity-0 group-hover:opacity-100 transition-opacity"
        aria-label={copied ? "Copied" : "Copy code"}
      >
        {copied ? (
          <Check className="w-4 h-4 text-[var(--status-success)]" />
        ) : (
          <Copy className="w-4 h-4" />
        )}
      </Button>
    </div>
  );
}

// ============================================================================
// Markdown Components
// ============================================================================

function stripLineSuffix(path: string): string {
  return path.replace(/:\d+(?::\d+)?$/, "");
}

function parseLocalFileHref(href: string | undefined): string | null {
  if (!href) return null;

  if (href.startsWith("file://")) {
    try {
      return stripLineSuffix(decodeURI(new URL(href).pathname));
    } catch {
      return stripLineSuffix(href.slice("file://".length));
    }
  }

  if (href.startsWith("/") && !href.startsWith("//")) {
    return stripLineSuffix(href);
  }

  return null;
}

function fileHref(path: string): string {
  return `file://${path}`;
}

function normalizePathForCompare(path: string): string {
  const normalized = path.replace(/\/+$/, "");
  return normalized || "/";
}

function isPathInsideWorkspace(path: string, workspaceRootPath: string): boolean {
  const normalizedPath = normalizePathForCompare(path);
  const normalizedRoot = normalizePathForCompare(workspaceRootPath);
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

function textFromReactNode(node: React.ReactNode): string {
  if (typeof node === "string" || typeof node === "number") {
    return String(node);
  }
  if (Array.isArray(node)) {
    return node.map(textFromReactNode).join("");
  }
  return "file";
}

async function openLocalFilePath(path: string): Promise<void> {
  try {
    await openPath(path);
    return;
  } catch (openError) {
    try {
      await revealItemInDir(path);
    } catch (revealError) {
      logger.warn("[chat] Failed to open local file link", {
        path,
        openError,
        revealError,
      });
    }
  }
}

/**
 * A file reference the user can see named but cannot open from here (2.6-a).
 *
 * Rendered INSTEAD of both the workspace open-menu link and the plain `file://`
 * anchor whenever the active environment is remote, which is what keeps `openPath` /
 * `revealItemInDir` / `open_agent_conversation_workspace_path` off the click path
 * entirely — the disabled-control variant would still leave a menu whose every item
 * is dead.
 */
function RemoteHostFileLink({
  path,
  children,
}: {
  path: string;
  children: React.ReactNode;
}) {
  return (
    <span
      className="inline-flex max-w-full items-baseline gap-0.5 align-baseline"
      data-testid="chat-remote-host-file-link"
    >
      <span
        className="min-w-0 break-words"
        style={{ color: "var(--text-secondary)" }}
        title={path}
      >
        {children}
      </span>
      <HostPathCopyButton path={path} testId="chat-remote-host-file-copy" />
    </span>
  );
}

function WorkspaceMarkdownFileLink({
  path,
  children,
}: {
  path: string;
  children: React.ReactNode;
}) {
  const fileLinkContext = useMessageFileLinkContext();
  if (!fileLinkContext) {
    return null;
  }

  const preferredTarget = resolvePreferredWorkspaceOpenTarget(
    fileLinkContext.targets,
    fileLinkContext.preferredTargetId,
  );
  if (!preferredTarget) {
    return null;
  }

  const linkLabel = textFromReactNode(children);
  const isOpening = fileLinkContext.openingPath === path;
  const displayedTarget =
    isOpening && fileLinkContext.openingTargetId
      ? fileLinkContext.targets.find((target) => target.id === fileLinkContext.openingTargetId) ??
        preferredTarget
      : preferredTarget;

  const openTarget = (targetId: string) => {
    fileLinkContext.onOpenPath(path, targetId);
  };

  return (
    <span className="inline-flex max-w-full items-baseline gap-0.5 align-baseline">
      <button
        type="button"
        className="min-w-0 cursor-pointer break-words p-0 text-left underline hover:no-underline disabled:cursor-default disabled:opacity-70"
        style={{ color: "var(--accent-primary)" }}
        onClick={() => openTarget(preferredTarget.id)}
        disabled={isOpening}
        aria-label={`Open ${linkLabel} in ${displayedTarget.label}`}
        data-testid="chat-workspace-file-link"
      >
        {children}
      </button>
      <DropdownMenu>
        <Tooltip>
          <DropdownMenuTrigger asChild>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-sm p-0 align-baseline transition-colors hover:bg-[var(--overlay-faint)] disabled:opacity-70"
                style={{ color: "var(--accent-primary)" }}
                disabled={isOpening}
                aria-label={`Open options for ${linkLabel}`}
                data-testid="chat-workspace-file-link-options"
              >
                {isOpening ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <ChevronDown className="h-3 w-3" />
                )}
              </button>
            </TooltipTrigger>
          </DropdownMenuTrigger>
          <TooltipContent side="top" className="text-xs">
            Open options
          </TooltipContent>
        </Tooltip>
        <DropdownMenuContent align="start" className="min-w-[180px]">
          {fileLinkContext.targets.map((target) => {
            const Icon =
              target.kind === "fileManager"
                ? FolderOpen
                : target.kind === "terminal"
                  ? TerminalIcon
                  : Code2;
            const selected = target.id === preferredTarget.id;
            const label =
              target.kind === "fileManager"
                ? `Reveal in ${target.label}`
                : `Open in ${target.label}`;
            return (
              <DropdownMenuItem
                key={target.id}
                onClick={() => openTarget(target.id)}
              >
                <Icon className="h-4 w-4" />
                <span>{label}</span>
                {selected ? <Check className="ml-auto h-3.5 w-3.5" /> : null}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
    </span>
  );
}

export function MarkdownLink({
  href,
  children,
  ...props
}: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const localFilePath = parseLocalFileHref(href);
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const fileLinkContext = useMessageFileLinkContext();
  const useWorkspaceFileLink = Boolean(
    localFilePath &&
      fileLinkContext &&
      fileLinkContext.targets.length > 0 &&
      isPathInsideWorkspace(localFilePath, fileLinkContext.workspaceRootPath),
  );

  const handleClick = useCallback(
    (event: React.MouseEvent<HTMLAnchorElement>) => {
      if (!localFilePath) return;
      event.preventDefault();
      // Belt-and-braces: the remote branch below already returns before this handler
      // can be bound to a rendered anchor, but `openLocalFilePath` opens a file on
      // THIS device and must never fire for a host path.
      if (isRemoteEnvironment) return;
      void openLocalFilePath(localFilePath);
    },
    [isRemoteEnvironment, localFilePath],
  );

  if (isRemoteEnvironment && localFilePath) {
    return <RemoteHostFileLink path={localFilePath}>{children}</RemoteHostFileLink>;
  }

  if (useWorkspaceFileLink && localFilePath) {
    return (
      <WorkspaceMarkdownFileLink path={localFilePath}>
        {children}
      </WorkspaceMarkdownFileLink>
    );
  }

  return (
    <a
      {...props}
      href={localFilePath ? fileHref(localFilePath) : href}
      target={localFilePath ? undefined : "_blank"}
      rel={localFilePath ? undefined : "noopener noreferrer"}
      className="underline hover:no-underline"
      style={{ color: "var(--accent-primary)" }} /* macOS Tahoe accent */
      onClick={handleClick}
    >
      {children}
    </a>
  );
}

export const markdownComponents = {
  a: MarkdownLink,
  code: ({
    className,
    children,
    ...props
  }: React.HTMLAttributes<HTMLElement>) => {
    const match = /language-(\w+)/.exec(className || "");
    const content = String(children);
    const isBlock = Boolean(match);
    const isMultiline = content.includes("\n");
    if (isBlock || isMultiline) {
      return (
        <CodeBlock language={match?.[1]}>{content.trim()}</CodeBlock>
      );
    }
    return (
      <code
        className="px-1 py-px rounded text-[0.75rem] break-words"
        style={{
          backgroundColor: "var(--overlay-faint)",
          color: "var(--text-primary)",
          fontFamily: "var(--font-mono)",
        }}
        {...props}
      >
        {children}
      </code>
    );
  },
  pre: ({ children }: React.HTMLAttributes<HTMLPreElement>) => <>{children}</>,
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => {
    if (looksLikeAsciiArt(children)) {
      const { className: _cls, style: _sty, ...rest } = props;
      return (
        <pre
          className="leading-relaxed [&:not(:last-child)]:mb-2 text-[0.75rem] overflow-x-auto"
          style={{ fontFamily: "var(--font-mono)", color: "var(--text-primary)" }}
          {...(rest as React.HTMLAttributes<HTMLPreElement>)}
        >
          {children}
        </pre>
      );
    }
    return (
      <p className="leading-relaxed [&:not(:last-child)]:mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
        {children}
      </p>
    );
  },
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1 className="text-lg font-bold mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2 className="text-base font-bold mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3 className="text-[0.9375rem] font-bold mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
      {children}
    </h3>
  ),
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="list-disc pl-4 mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal pl-4 mb-2" style={{ maxWidth: PROSE_MAX_WIDTH }} {...props}>
      {children}
    </ol>
  ),
  li: ({ children, ...props }: React.LiHTMLAttributes<HTMLLIElement>) => (
    <li className="mb-1" {...props}>
      {children}
    </li>
  ),
  strong: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <strong className="font-semibold" {...props}>
      {children}
    </strong>
  ),
  em: ({ children, ...props }: React.HTMLAttributes<HTMLElement>) => (
    <em className="italic" {...props}>
      {children}
    </em>
  ),
  hr: ({ ...props }: React.HTMLAttributes<HTMLHRElement>) => (
    <hr
      style={{
        /* macOS Tahoe: very subtle separator */
        border: "none",
        borderTop: "1px solid var(--overlay-weak)",
        marginTop: "12px",
        marginBottom: "12px",
      }}
      {...props}
    />
  ),
  // Table support - macOS Tahoe flat styling with horizontal scroll
  table: ({ children, ...props }: React.TableHTMLAttributes<HTMLTableElement>) => (
    <div
      className="overflow-x-auto my-3 rounded-lg"
      style={{
        /* macOS Tahoe: subtle background for table container */
        backgroundColor: "var(--bg-surface)",
      }}
    >
      <table
        className="w-full text-[0.75rem] border-collapse"
        {...props}
      >
        {children}
      </table>
    </div>
  ),
  thead: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <thead
      style={{
        /* macOS Tahoe: slightly elevated header */
        backgroundColor: "var(--bg-hover)",
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
      style={{
        /* macOS Tahoe: very subtle row separator */
        borderBottom: "1px solid var(--overlay-faint)",
      }}
      {...props}
    >
      {children}
    </tr>
  ),
  th: ({ children, ...props }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
    <th
      className="px-3 py-2 text-left font-medium text-[0.6875rem] uppercase tracking-wide"
      style={{
        color: "var(--text-secondary)",
      }}
      {...props}
    >
      {children}
    </th>
  ),
  td: ({ children, ...props }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
    <td
      className="px-3 py-2"
      style={{
        color: "var(--text-primary)",
      }}
      {...props}
    >
      {children}
    </td>
  ),
};
