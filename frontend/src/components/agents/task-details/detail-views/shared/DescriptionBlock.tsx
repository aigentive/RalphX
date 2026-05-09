/**
 * DescriptionBlock - Clean description text with empty state
 */
import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";

interface DescriptionBlockProps {
  description: string | null | undefined;
  testId?: string;
}

/**
 * Some task descriptions are persisted as JSON-encoded strings without their
 * outer quotes, leaving literal `\n` and `\"` escape sequences in the body.
 * If we detect those without real newlines, decode the standard escapes so
 * paragraph breaks and quotes render through markdown instead of bleeding
 * into the rendered output as visible characters.
 */
function unescapeIfEncoded(text: string): string {
  const hasEscapedNewline = /\\n/.test(text);
  const hasRealNewline = /\n/.test(text);
  const hasEscapedQuote = /\\"/.test(text);
  if (hasRealNewline || (!hasEscapedNewline && !hasEscapedQuote)) {
    return text;
  }
  return text
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t")
    .replace(/\\"/g, '"')
    .replace(/\\\\/g, "\\");
}

export function DescriptionBlock({ description, testId }: DescriptionBlockProps) {
  const decoded = useMemo(
    () => (description ? unescapeIfEncoded(description) : null),
    [description],
  );

  if (!decoded) {
    return (
      <p className="text-[0.8125rem] italic text-text-primary/30">
        No description provided
      </p>
    );
  }

  return (
    <div
      data-testid={testId}
      className="text-[0.8125rem] text-text-primary leading-relaxed"
      style={{ wordBreak: "break-word" }}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {decoded}
      </ReactMarkdown>
    </div>
  );
}
