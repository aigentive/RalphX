/* eslint-disable react-refresh/only-export-components */
/**
 * MessageItem.markdown - Markdown rendering components for MessageItem
 *
 * Contains:
 * - CodeBlock: Code display with copy functionality
 * - markdownComponents: ReactMarkdown component overrides
 */

import React, { useState, useCallback } from "react";
import { Copy, Check } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

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
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
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

export function MarkdownLink({
  href,
  children,
  ...props
}: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const localFilePath = parseLocalFileHref(href);

  const handleClick = useCallback(
    (event: React.MouseEvent<HTMLAnchorElement>) => {
      if (!localFilePath) return;
      event.preventDefault();
      void openPath(localFilePath);
    },
    [localFilePath],
  );

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
    const isBlock = Boolean(match);
    if (isBlock) {
      return (
        <CodeBlock language={match?.[1]}>{String(children).trim()}</CodeBlock>
      );
    }
    return (
      <code
        className="px-1 py-px rounded text-[0.75rem] break-all"
        style={{
          /* Soft inline code chip — distinguishable as code without
             dominating dense paragraphs that contain many spans. */
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
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
    <p className="leading-relaxed [&:not(:last-child)]:mb-2" {...props}>
      {children}
    </p>
  ),
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1 className="text-lg font-bold mb-2" {...props}>
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2 className="text-base font-bold mb-2" {...props}>
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3 className="text-[0.9375rem] font-bold mb-2" {...props}>
      {children}
    </h3>
  ),
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="list-disc pl-4 mb-2" {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal pl-4 mb-2" {...props}>
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
        className="text-[0.75rem] border-collapse"
        style={{ minWidth: "max-content" }} /* Prevent column shrinking */
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
        whiteSpace: "nowrap", /* Prevent text wrapping */
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
        whiteSpace: "nowrap", /* Prevent text wrapping */
      }}
      {...props}
    >
      {children}
    </td>
  ),
};
