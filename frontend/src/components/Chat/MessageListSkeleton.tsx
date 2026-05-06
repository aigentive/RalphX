/**
 * MessageListSkeleton - Premium loading state for chat message list
 *
 * Design: Native Mac app elegance with warm orange accent.
 * Centered, minimal, with organic breathing animation.
 */

import { withAlpha } from "@/lib/theme-colors";

const skeletonStyles = `
@keyframes skeleton-breathe {
  0%, 100% {
    opacity: 0.4;
    transform: scale(1);
  }
  50% {
    opacity: 0.7;
    transform: scale(1.02);
  }
}

@keyframes skeleton-pulse-ring {
  0% {
    transform: scale(0.95);
    opacity: 0.5;
  }
  50% {
    transform: scale(1);
    opacity: 0.8;
  }
  100% {
    transform: scale(0.95);
    opacity: 0.5;
  }
}

@keyframes skeleton-fade-in {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.skeleton-line {
  background: linear-gradient(
    90deg,
    var(--overlay-faint) 0%,
    var(--overlay-weak) 50%,
    var(--overlay-faint) 100%
  );
  border-radius: 6px;
  animation: skeleton-breathe 2.4s ease-in-out infinite;
}

.skeleton-line-accent {
  background: linear-gradient(
    90deg,
    color-mix(in srgb, var(--accent-primary) 6%, transparent) 0%,
    color-mix(in srgb, var(--accent-primary) 12%, transparent) 50%,
    color-mix(in srgb, var(--accent-primary) 6%, transparent) 100%
  );
  border-radius: 6px;
  animation: skeleton-breathe 2.4s ease-in-out infinite;
}

.skeleton-avatar {
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--accent-primary) 15%, transparent) 0%,
    color-mix(in srgb, var(--accent-primary) 8%, transparent) 100%
  );
  animation: skeleton-pulse-ring 2.4s ease-in-out infinite;
}
`;

export function MessageListSkeleton() {
  return (
    <>
      <style>{skeletonStyles}</style>
      <div
        data-testid="chat-panel-loading"
        className="flex flex-col items-center justify-center h-full px-6"
        style={{
          animation: "skeleton-fade-in 0.3s ease-out",
        }}
      >
        {/* Central loading indicator */}
        <div className="flex flex-col items-center gap-5 max-w-[280px] w-full">
          {/* Animated icon container */}
          <div
            className="skeleton-avatar w-10 h-10 rounded-xl flex items-center justify-center"
            style={{
              boxShadow: `0 0 20px ${withAlpha("var(--accent-primary)", 10)}`,
            }}
          >
            {/* Three dots in a subtle arrangement */}
            <div className="flex items-center gap-1">
              {[0, 1, 2].map((i) => (
                <div
                  key={i}
                  className="w-1.5 h-1.5 rounded-full"
                  style={{
                    backgroundColor: withAlpha("var(--accent-primary)", 60),
                    animation: `skeleton-breathe 1.6s ease-in-out infinite`,
                    animationDelay: `${i * 0.15}s`,
                  }}
                />
              ))}
            </div>
          </div>

          {/* Abstract message preview lines */}
          <div className="flex flex-col gap-2.5 w-full">
            {/* Assistant-style line (left aligned, longer) */}
            <div className="flex items-center gap-2">
              <div
                className="skeleton-avatar w-5 h-5 rounded-full shrink-0"
              />
              <div
                className="skeleton-line h-3 flex-1"
                style={{ animationDelay: "0.1s", maxWidth: "85%" }}
              />
            </div>

            {/* User-style line (right aligned, shorter, accent) */}
            <div className="flex justify-end">
              <div
                className="skeleton-line-accent h-3"
                style={{ animationDelay: "0.2s", width: "60%" }}
              />
            </div>

            {/* Another assistant line */}
            <div className="flex items-center gap-2">
              <div
                className="skeleton-avatar w-5 h-5 rounded-full shrink-0"
                style={{ animationDelay: "0.25s" }}
              />
              <div
                className="skeleton-line h-3"
                style={{ animationDelay: "0.3s", width: "70%" }}
              />
            </div>
          </div>

          {/* Subtle loading text */}
          <p
            className="text-[0.6875rem] tracking-wide uppercase"
            style={{
              color: "var(--text-muted)",
              fontFamily: "var(--font-body)",
              letterSpacing: "0.08em",
              animation: "skeleton-breathe 2.4s ease-in-out infinite",
              animationDelay: "0.4s",
            }}
          >
            Loading conversation
          </p>
        </div>
      </div>
    </>
  );
}
