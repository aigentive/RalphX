import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

import { TeamContextBar } from "./TeamContextBar";
import { useTeamStore } from "@/stores/teamStore";

const CONTEXT_KEY = "session:test-team-bar";

beforeEach(() => {
  useTeamStore.setState({
    activeTeams: {
      [CONTEXT_KEY]: {
        teamName: "research",
        leadName: "lead",
        teammates: {
          "researcher-a": {
            name: "researcher-a",
            color: "var(--accent-primary)",
            model: "claude-opus-4",
            roleDescription: "research",
            status: "running",
            currentActivity: "scanning sources",
            tokensUsed: 12_345,
            estimatedCostUsd: 0.05,
            conversationId: null,
          },
          "researcher-b": {
            name: "researcher-b",
            color: "var(--accent-primary)",
            model: "gpt-5.4",
            roleDescription: "synthesis",
            status: "spawning",
            currentActivity: null,
            tokensUsed: 0,
            estimatedCostUsd: 0,
            conversationId: null,
          },
        },
        messages: [],
        totalTokens: 12_345,
        totalEstimatedCostUsd: 0.05,
        createdAt: new Date().toISOString(),
      },
    },
    pendingPlans: {},
    artifactVersion: {},
  } as Partial<ReturnType<typeof useTeamStore.getState>>);
});

describe("TeamContextBar", () => {
  it("renders the lead summary row with active and running counts", () => {
    render(<TeamContextBar contextKey={CONTEXT_KEY} activeFilter="lead" />);
    expect(screen.getByText(/2 active/)).toBeInTheDocument();
    expect(screen.getByText(/2 running/)).toBeInTheDocument();
  });

  it("renders the historical badge in summary mode when isHistorical=true", () => {
    render(<TeamContextBar contextKey={CONTEXT_KEY} activeFilter="lead" isHistorical />);
    expect(screen.getByText("Session ended")).toBeInTheDocument();
  });

  it("renders teammate detail row for a specific filter", () => {
    render(<TeamContextBar contextKey={CONTEXT_KEY} activeFilter="researcher-a" />);
    expect(screen.getByText("researcher-a")).toBeInTheDocument();
    expect(screen.getByText("claude-opus-4")).toBeInTheDocument();
  });
});
