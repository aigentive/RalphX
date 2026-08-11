/**
 * TextBubble - Chat message text bubble with copy functionality
 *
 * Renders text content with:
 * - User vs assistant styling
 * - Markdown rendering for user and assistant messages
 */

import { memo, Suspense, useEffect, useRef, useState } from "react";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { cn } from "@/lib/utils";
import { markdownComponents } from "./MessageItem.markdown";

interface TextBubbleProps {
  text: string;
  isUser: boolean;
  isStreaming?: boolean;
}

interface MarkdownContentProps {
  text: string;
}

const LazyMarkdownContent = lazyWithRetry(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] = await Promise.all([
    import("react-markdown"),
    import("remark-gfm"),
  ]);

  return {
    default: memo(function MarkdownContent({ text }: MarkdownContentProps) {
      return (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {text}
        </ReactMarkdown>
      );
    }),
  };
});

const STREAMING_MARKDOWN_UPDATE_MS = 200;

export function TextBubble({ text, isUser, isStreaming = false }: TextBubbleProps) {
  const canHydrateMarkdown = useAfterPaintReady();
  const markdownText = useStreamingMarkdownText(text, isStreaming);

  return (
    <div
      data-testid={isUser ? "text-bubble-user" : "text-bubble-assistant"}
      className={cn(
        "text-[0.8125rem] leading-relaxed break-words",
        isUser ? "w-fit px-3 py-2 rounded-xl" : "w-full px-0 py-0 rounded-none",
        isUser ? "self-end" : "self-start"
      )}
      style={{
        maxWidth: isUser ? "min(85%, 620px)" : undefined,
        // The bubble already caps its own width; re-capping markdown blocks
        // against the fit-content bubble width breaks short messages mid-word.
        ...(isUser ? { ["--chat-prose-max-width" as string]: "none" } : {}),
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
          <Suspense fallback={<PlainTextContent text={markdownText} />}>
            <LazyMarkdownContent text={markdownText} />
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

function useAfterPaintReady(): boolean {
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
  }, []);

  return isReady;
}

function useStreamingMarkdownText(text: string, isStreaming: boolean): string {
  const [renderedText, setRenderedText] = useState(text);
  const renderedTextRef = useRef(text);
  const latestTextRef = useRef(text);
  const flushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    latestTextRef.current = text;

    if (!isStreaming) {
      if (flushTimerRef.current !== null) {
        clearTimeout(flushTimerRef.current);
        flushTimerRef.current = null;
      }
      if (renderedTextRef.current !== text) {
        renderedTextRef.current = text;
        setRenderedText(text);
      }
      return;
    }

    if (renderedTextRef.current === text || flushTimerRef.current !== null) {
      return;
    }

    flushTimerRef.current = setTimeout(() => {
      flushTimerRef.current = null;
      const nextText = latestTextRef.current;
      if (renderedTextRef.current !== nextText) {
        renderedTextRef.current = nextText;
        setRenderedText(nextText);
      }
    }, STREAMING_MARKDOWN_UPDATE_MS);
  }, [isStreaming, text]);

  useEffect(() => () => {
    if (flushTimerRef.current !== null) {
      clearTimeout(flushTimerRef.current);
    }
  }, []);

  return renderedText;
}
