/**
 * TextBubble - Chat message text bubble with copy functionality
 *
 * Renders text content with:
 * - User vs assistant styling
 * - Markdown rendering for user and assistant messages
 */

import { lazy, Suspense, useEffect, useState } from "react";
import { cn } from "@/lib/utils";
import { markdownComponents } from "./MessageItem.markdown";

interface TextBubbleProps {
  text: string;
  isUser: boolean;
}

interface MarkdownContentProps {
  text: string;
}

const LazyMarkdownContent = lazy(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] = await Promise.all([
    import("react-markdown"),
    import("remark-gfm"),
  ]);

  return {
    default: function MarkdownContent({ text }: MarkdownContentProps) {
      return (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {text}
        </ReactMarkdown>
      );
    },
  };
});

export function TextBubble({ text, isUser }: TextBubbleProps) {
  const canHydrateMarkdown = useAfterPaintReady(text);

  return (
    <div
      data-testid={isUser ? "text-bubble-user" : "text-bubble-assistant"}
      className={cn(
        "w-fit text-[0.8125rem] leading-relaxed break-words",
        isUser ? "px-3 py-2 rounded-xl" : "px-0 py-0 rounded-none",
        isUser ? "self-end" : "self-start"
      )}
      style={{
        maxWidth: "min(85%, 620px)",
        background: isUser ? "var(--chat-user-bubble-bg)" : "transparent",
        color: isUser ? "var(--chat-user-bubble-text)" : "var(--text-primary)",
        borderWidth: isUser ? "1px" : "0",
        borderStyle: isUser ? "solid" : "none",
        borderColor: isUser ? "var(--chat-user-bubble-border)" : "transparent",
        boxShadow: "none",
      }}
    >
      <div className="max-w-none overflow-hidden [&>p]:mb-0">
        {canHydrateMarkdown ? (
          <Suspense fallback={<PlainTextContent text={text} />}>
            <LazyMarkdownContent text={text} />
          </Suspense>
        ) : (
          <PlainTextContent text={text} />
        )}
      </div>
    </div>
  );
}

function PlainTextContent({ text }: MarkdownContentProps) {
  return <span className="whitespace-pre-wrap">{text}</span>;
}

function useAfterPaintReady(key: string): boolean {
  const [isReady, setIsReady] = useState(false);

  useEffect(() => {
    setIsReady(false);
    let timer: number | null = null;
    let frame: number | null = null;
    frame = window.requestAnimationFrame(() => {
      frame = null;
      timer = window.setTimeout(() => {
        timer = null;
        setIsReady(true);
      }, 0);
    });

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      if (timer !== null) {
        window.clearTimeout(timer);
      }
    };
  }, [key]);

  return isReady;
}
