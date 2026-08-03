import React, { useEffect, useRef, useState } from "react";
import { colors } from "./shared.constants";

const BOTTOM_EPSILON_PX = 2;

function ThinkingText({ text }: { text: string }) {
  return text.split(/(\*\*[^*]+\*\*)/g).map((part, index) => {
    if (part.startsWith("**") && part.endsWith("**")) {
      return <strong key={index}>{part.slice(2, -2)}</strong>;
    }
    return part;
  });
}

export const ThinkingWidget = React.memo(function ThinkingWidget({ text, compact = false }: {
  text: string;
  compact?: boolean;
}) {
  const [hydrated, setHydrated] = useState(false);
  const [pinned, setPinned] = useState(true);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const frame = requestAnimationFrame(() => {
      timer = setTimeout(() => setHydrated(true), 0);
    });
    return () => {
      cancelAnimationFrame(frame);
      if (timer !== null) clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    const element = scrollRef.current;
    if (hydrated && pinned && element) element.scrollTop = element.scrollHeight;
  }, [hydrated, pinned, text]);

  return hydrated ? (
        <div ref={scrollRef} data-testid="thinking-scroll-body"
          onScroll={(event) => {
            const element = event.currentTarget;
            setPinned(element.scrollHeight - element.scrollTop - element.clientHeight <= BOTTOM_EPSILON_PX);
          }}
          style={{
            maxHeight: "15.5em", overflowY: "auto", whiteSpace: "pre-wrap",
            fontFamily: "var(--font-mono)", fontSize: compact ? 10 : 11, lineHeight: 1.55,
            color: colors.textSecondary, padding: "2px 2px 2px 4px",
          }}>
          <ThinkingText text={text} />
        </div>
      ) : <div data-testid="thinking-widget-shell" style={{ minHeight: "2em" }} />;
});
