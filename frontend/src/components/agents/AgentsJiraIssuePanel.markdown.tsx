import type { HTMLAttributes } from "react";

import { markdownComponents } from "@/components/Chat/MessageItem.markdown";

export const jiraMarkdownComponents = {
  ...markdownComponents,
  p: ({ children, ...props }: HTMLAttributes<HTMLParagraphElement>) => (
    <p
      className="mb-3 last:mb-0 leading-relaxed"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </p>
  ),
  h1: ({ children, ...props }: HTMLAttributes<HTMLHeadingElement>) => (
    <h1
      className="mb-3 mt-0 text-lg font-semibold"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: HTMLAttributes<HTMLHeadingElement>) => (
    <h2
      className="mb-2 mt-4 text-base font-semibold first:mt-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: HTMLAttributes<HTMLHeadingElement>) => (
    <h3
      className="mb-2 mt-4 text-sm font-semibold first:mt-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h3>
  ),
  h4: ({ children, ...props }: HTMLAttributes<HTMLHeadingElement>) => (
    <h4
      className="mb-1.5 mt-3 text-[0.8125rem] font-semibold first:mt-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </h4>
  ),
  ul: ({ children, ...props }: HTMLAttributes<HTMLUListElement>) => (
    <ul
      className="mb-3 list-disc space-y-1 pl-5 last:mb-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: HTMLAttributes<HTMLOListElement>) => (
    <ol
      className="mb-3 list-decimal space-y-1 pl-5 last:mb-0"
      style={{ color: "var(--text-primary)" }}
      {...props}
    >
      {children}
    </ol>
  ),
  li: ({ children, ...props }: HTMLAttributes<HTMLLIElement>) => (
    <li className="pl-1 leading-relaxed" style={{ color: "var(--text-primary)" }} {...props}>
      {children}
    </li>
  ),
  strong: ({ children, ...props }: HTMLAttributes<HTMLElement>) => (
    <strong className="font-semibold" style={{ color: "var(--text-primary)" }} {...props}>
      {children}
    </strong>
  ),
  code: ({
    className,
    children,
    style: _style,
    ...props
  }: HTMLAttributes<HTMLElement>) => {
    const content = String(children);
    const isBlock = Boolean(className?.includes("language-")) || content.includes("\n");
    if (isBlock) {
      return (
        <code
          className="block min-w-full px-4 py-3 text-[0.78125rem] leading-relaxed"
          style={{
            color: "var(--text-primary)",
            fontFamily: "var(--font-mono)",
            whiteSpace: "pre",
          }}
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code
        className="break-words rounded px-1 py-px text-[0.75rem]"
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
  pre: ({
    children,
    className: _className,
    style: _style,
    ...props
  }: HTMLAttributes<HTMLPreElement>) => (
    <pre
      className="my-3 overflow-x-auto rounded-md text-left"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
      {...props}
    >
      {children}
    </pre>
  ),
};
