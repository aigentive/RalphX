/**
 * WelcomeScreen - Impressive animated welcome screen for first-run experience
 *
 * "Agent Constellation" design: animated agent network showing AI orchestration
 * with 4 orbiting nodes around a pulsing central hub, connected by glowing paths
 * with traveling data particles.
 *
 * Anti-AI-Slop: No purple/blue gradients, warm orange #ff6b35, SF Pro typography
 */

import { useEffect, useState } from "react";
import { Settings, Sparkles, X } from "lucide-react";
import AgentConstellation from "./AgentConstellation";

interface WelcomeScreenProps {
  onCreateProject: () => void;
  onSetupProviders?: () => void;
  providerSetupRequired?: boolean;
  hasProjects?: boolean;
  /** Optional callback when closing manually-opened welcome screen (via ⌘⇧W or Escape) */
  onClose?: (() => void) | undefined;
}

export default function WelcomeScreen({
  onCreateProject,
  onSetupProviders,
  providerSetupRequired = false,
  hasProjects = false,
  onClose,
}: WelcomeScreenProps) {
  // Track idle state for keyboard hint pulse animation
  const [isIdle, setIsIdle] = useState(false);

  useEffect(() => {
    // Start idle pulse animation after 3 seconds
    const idleTimer = setTimeout(() => setIsIdle(true), 3000);
    return () => clearTimeout(idleTimer);
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const activeElement = document.activeElement;
      if (
        activeElement instanceof HTMLInputElement ||
        activeElement instanceof HTMLTextAreaElement ||
        activeElement?.hasAttribute("contenteditable")
      ) {
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "n") {
        event.preventDefault();
        if (providerSetupRequired) {
          onSetupProviders?.();
          return;
        }
        onCreateProject();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onCreateProject, onSetupProviders, providerSetupRequired]);

  const action = providerSetupRequired ? onSetupProviders : onCreateProject;
  const Icon = providerSetupRequired ? Settings : Sparkles;
  const actionLabel = providerSetupRequired
    ? "Set Up Provider"
    : hasProjects
      ? "Continue"
      : "Start Your First Project";

  return (
    <div
      className="flex-1 flex flex-col items-center justify-center relative overflow-hidden"
      style={{ backgroundColor: "var(--bg-base)" }}
      data-testid="welcome-screen"
    >
      {/* Close button - only shown when manually opened (onClose provided) */}
      {onClose && (
        <button
          onClick={onClose}
          className="absolute top-6 right-6 z-50 p-2 rounded-lg transition-all duration-200 hover:scale-105 active:scale-95"
          style={{
            backgroundColor: "var(--overlay-weak)",
            border: "1px solid var(--overlay-moderate)",
            color: "var(--text-secondary)",
          }}
          aria-label="Close welcome screen"
          data-testid="close-welcome-screen"
        >
          <X className="w-5 h-5" />
        </button>
      )}

      {/* Agent Constellation background - full screen animated network */}
      <div className="absolute inset-0 z-0">
        <AgentConstellation />
      </div>

      {/* Gradient overlay for text readability — fades the canvas color
          (--bg-base) toward transparent so the central text reads well in
          every theme. Avoid hardcoded dark colors that turn into a smudge
          on light/HC. */}
      <div
        className="absolute inset-0 pointer-events-none z-30"
        style={{
          background:
            "radial-gradient(circle at center, color-mix(in srgb, var(--bg-base) 85%, transparent) 0%, color-mix(in srgb, var(--bg-base) 60%, transparent) 180px, color-mix(in srgb, var(--bg-base) 20%, transparent) 280px, transparent 380px)",
          isolation: "isolate",
        }}
      />

      {/* Content container - floats above the constellation */}
      <div className="relative z-40 flex flex-col items-center px-8 max-w-4xl w-full">
        {/* Hero section */}
        <div
          className="text-center mb-14 hero-section"
          style={{ animation: "fadeSlideIn 0.6s ease-out forwards" }}
        >
          {/* RalphX title with accent X and glow */}
          <h1
            className="text-7xl font-bold tracking-tight mb-3"
            style={{
              fontFamily: "var(--font-display)",
              color: "var(--text-primary)",
              textShadow: "0 0 60px rgba(255, 107, 53, 0.2)",
            }}
          >
            Ralph
            <span
              style={{
                color: "var(--accent-primary)",
                textShadow: "0 0 30px rgba(255, 107, 53, 0.5)",
              }}
            >
              X
            </span>
          </h1>

          {/* Tagline - updated per plan */}
          <p
            className="text-xl font-light"
            style={{
              fontFamily: "var(--font-body)",
              color: "var(--text-secondary)",
              letterSpacing: "var(--tracking-wide)",
            }}
          >
            {providerSetupRequired ? "Choose your agent harness." : "Describe it. Ship it."}
          </p>
        </div>

        {/* CTA section */}
        <div
          className="flex flex-col items-center gap-4 cta-section"
          style={{
            animation: "fadeSlideIn 0.6s ease-out 0.2s forwards",
            opacity: 0,
          }}
        >
          {/* Primary CTA button with glow */}
          <button
            onClick={action}
            className="group flex items-center gap-2 px-5 py-2.5 rounded-lg text-sm font-semibold transition-all duration-300 hover:scale-[1.02] active:scale-[0.98] cta-button"
            style={{
              backgroundColor: "var(--accent-primary)",
              color: "#fff",
              fontFamily: "var(--font-body)",
              boxShadow:
                "0 0 20px rgba(255, 107, 53, 0.3), 0 0 40px rgba(255, 107, 53, 0.1)",
            }}
            data-testid="create-first-project-button"
          >
            <Icon className="w-4 h-4 transition-transform group-hover:rotate-12" />
            {actionLabel}
          </button>

          {/* Keyboard shortcut hint with idle pulse */}
          {!providerSetupRequired && (
            <p
              className={`text-sm transition-all duration-300 ${isIdle ? "keyboard-hint-pulse" : ""}`}
              style={{
                color: "var(--text-muted)",
                fontFamily: "var(--font-body)",
              }}
            >
              Press{" "}
              <kbd
                className="px-2 py-0.5 rounded text-xs font-medium"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  color: "var(--text-secondary)",
                  border: "1px solid var(--border-default)",
                }}
              >
                ⌘N
              </kbd>{" "}
              to create a project
            </p>
          )}
        </div>
      </div>

      {/* CSS animations */}
      <style>{`
        /* Staggered fade-in animation for content sections */
        @keyframes fadeSlideIn {
          from {
            opacity: 0;
            transform: translateY(20px);
          }
          to {
            opacity: 1;
            transform: translateY(0);
          }
        }

        /* Button glow pulse animation */
        @keyframes glowPulse {
          0%, 100% {
            box-shadow: 0 0 20px rgba(255, 107, 53, 0.3), 0 0 40px rgba(255, 107, 53, 0.1);
          }
          50% {
            box-shadow: 0 0 30px rgba(255, 107, 53, 0.5), 0 0 60px rgba(255, 107, 53, 0.2);
          }
        }

        /* Keyboard hint pulse animation (after 3+ seconds idle) */
        @keyframes keyboardHintPulse {
          0%, 100% {
            opacity: 0.6;
            transform: scale(1);
          }
          50% {
            opacity: 1;
            transform: scale(1.02);
          }
        }

        .cta-button {
          animation: glowPulse 3s ease-in-out infinite;
        }

        .keyboard-hint-pulse {
          animation: keyboardHintPulse 2s ease-in-out infinite;
        }
      `}</style>
    </div>
  );
}
