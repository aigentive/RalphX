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
import { CheckCircle2, Plug, Settings, Sparkles, X } from "lucide-react";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";

import AgentConstellation from "./AgentConstellation";

interface WelcomeScreenProps {
  onCreateProject: () => void;
  onSetupProviders?: () => void;
  onSetupIntegrations?: () => void;
  providerSetupRequired?: boolean;
  hasProjects?: boolean;
  /** Optional callback when closing manually-opened welcome screen (via ⌘⇧W or Escape) */
  onClose?: (() => void) | undefined;
}

export default function WelcomeScreen({
  onCreateProject,
  onSetupProviders,
  onSetupIntegrations,
  providerSetupRequired = false,
  hasProjects = false,
  onClose,
}: WelcomeScreenProps) {
  // Track idle state for keyboard hint pulse animation
  const [isIdle, setIsIdle] = useState(false);

  /**
   * Project creation is host-impossible from a remote client (2.6-a): the wizard's
   * folder picker reads a filesystem this device cannot see. The empty state stays —
   * a remote session with no visible projects still needs an explanation — but the
   * CTA and its ⌘N shortcut are removed rather than left to fail on click.
   */
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const projectCreationBlocked =
    isRemoteEnvironment && !providerSetupRequired && !hasProjects;

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
        if (projectCreationBlocked) return;
        onCreateProject();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    onCreateProject,
    onSetupProviders,
    projectCreationBlocked,
    providerSetupRequired,
  ]);

  const action = providerSetupRequired
    ? onSetupProviders
    : hasProjects
      ? (onClose ?? onCreateProject)
      : onCreateProject;
  const Icon = providerSetupRequired ? Settings : Sparkles;
  const actionLabel = providerSetupRequired
    ? "Set Up Provider"
    : hasProjects
      ? "Continue"
      : "Start Your First Project";
  const providerStepStatus = providerSetupRequired ? "current" : "complete";
  const projectStepStatus = hasProjects
    ? "complete"
    : providerSetupRequired
      ? "pending"
      : "current";
  const providerStepSubtitle = providerSetupRequired
    ? "Choose your agent harness."
    : "Agent harness ready.";
  const projectStepSubtitle = hasProjects
    ? "Project workspace ready."
    : "Create your first project.";
  const providerStepCurrent = providerStepStatus === "current";
  const projectStepCurrent = projectStepStatus === "current";

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
            The best way to ship software with AI
          </p>
        </div>

        <div
          data-testid="welcome-setup-steps"
          className="mb-8 flex w-full max-w-2xl items-center justify-center gap-2"
          style={{ animation: "fadeSlideIn 0.6s ease-out 0.1s forwards" }}
        >
          <div
            data-testid="welcome-provider-step"
            data-current={providerStepCurrent ? "true" : "false"}
            data-status={providerStepStatus}
            className="flex min-w-0 flex-1 items-start gap-2 rounded-md border px-3 py-2 text-sm"
            style={{
              backgroundColor: providerStepCurrent
                ? "var(--bg-elevated)"
                : "var(--bg-surface)",
              borderColor: providerStepCurrent
                ? "var(--accent-primary)"
                : "var(--border-subtle)",
              color: providerStepCurrent
                ? "var(--text-primary)"
                : "var(--text-secondary)",
            }}
          >
            {providerStepCurrent ? (
              <Settings className="mt-0.5 h-4 w-4 shrink-0" />
            ) : (
              <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-[var(--status-success)]" />
            )}
            <span className="min-w-0">
              <span className="block truncate font-medium">Provider</span>
              <span
                className="block truncate text-[0.6875rem]"
                style={{ color: "var(--text-muted)" }}
              >
                {providerStepSubtitle}
              </span>
            </span>
          </div>
          <div
            data-testid="welcome-project-step"
            data-current={projectStepCurrent ? "true" : "false"}
            data-status={projectStepStatus}
            className="flex min-w-0 flex-1 items-start gap-2 rounded-md border px-3 py-2 text-sm"
            style={{
              backgroundColor: projectStepCurrent
                ? "var(--bg-elevated)"
                : "var(--bg-surface)",
              borderColor: projectStepCurrent
                ? "var(--accent-primary)"
                : "var(--border-subtle)",
              color: projectStepCurrent
                ? "var(--text-primary)"
                : "var(--text-secondary)",
            }}
          >
            {hasProjects ? (
              <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-[var(--status-success)]" />
            ) : (
              <Sparkles
                className="mt-0.5 h-4 w-4 shrink-0"
                style={{
                  color: projectStepCurrent
                    ? "var(--accent-primary)"
                    : "currentColor",
                }}
              />
            )}
            <span className="min-w-0">
              <span className="block truncate font-medium">Project</span>
              <span
                className="block truncate text-[0.6875rem]"
                style={{ color: "var(--text-muted)" }}
              >
                {projectStepSubtitle}
              </span>
            </span>
          </div>
          <button
            type="button"
            data-testid="welcome-integrations-step"
            data-status="optional"
            className="flex min-w-0 flex-1 items-start gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors hover:bg-[var(--bg-hover)]"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-subtle)",
              color: "var(--text-secondary)",
            }}
            onClick={onSetupIntegrations}
          >
            <Plug className="mt-0.5 h-4 w-4 shrink-0" />
            <span className="min-w-0">
              <span className="block truncate font-medium">Atlassian</span>
              <span
                className="block truncate text-[0.6875rem]"
                style={{ color: "var(--text-muted)" }}
              >
                Optional Jira and Confluence context.
              </span>
            </span>
          </button>
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
          {projectCreationBlocked ? (
            <p
              className="max-w-[420px] text-center text-sm"
              style={{ color: "var(--text-muted)", fontFamily: "var(--font-body)" }}
              data-testid="welcome-remote-no-create"
            >
              Projects are created on the host Mac. Switch to This Mac to add one.
            </p>
          ) : (
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
          )}

          {/* Keyboard shortcut hint with idle pulse */}
          {!providerSetupRequired && !hasProjects && !projectCreationBlocked && (
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
