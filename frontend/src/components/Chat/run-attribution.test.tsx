import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AgentRunAttribution } from "@/api/agent-runs";
import { RunAttributionWidget } from "./RunAttributionWidget";
import { roleVerb, workedDurationLabel } from "./run-attribution";

function attribution(overrides: Partial<AgentRunAttribution> = {}): AgentRunAttribution {
  return {
    id: "run-1", conversationId: "c", status: "completed", startedAt: "2026-07-31T00:00:00Z", completedAt: "2026-07-31T00:00:42Z",
    harness: "codex", upstreamProvider: "openai", providerProfile: null, providerSessionId: null,
    logicalModel: null, effectiveModelId: null, logicalEffort: null, effectiveEffort: null, serviceTier: null,
    approvalPolicy: null, sandboxMode: null, inputTokens: 11, outputTokens: 22, cacheCreationTokens: 33,
    cacheReadTokens: null, estimatedUsd: 0.0123, runChainId: null, actionKind: "workspace_review",
    personaSlug: null, agentName: "Reviewer", launchRole: "workspace_reviewer", runtimeSource: "role_default",
    ...overrides,
  };
}

describe("run attribution helpers", () => {
  it("maps feedback roles and formats duration", () => {
    expect(roleVerb("workspace_reviewer")).toBe("Reviewer");
    expect(roleVerb("workspace_repair")).toBe("Fixer");
    expect(roleVerb(null)).toBe("Agent");
    expect(workedDurationLabel("2026-07-31T00:00:00Z", "2026-07-31T00:01:04Z")).toBe("1m 4s");
  });
});

describe("RunAttributionWidget", () => {
  it("uses the shared attribution for both collapsed and expanded values", async () => {
    const user = userEvent.setup();
    render(<RunAttributionWidget runId="run-1" startedAt="2026-07-31T00:00:10Z" completedAt="2026-07-31T00:00:12Z" attribution={attribution()} />);

    expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Reviewer worked for 42s");
    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(await screen.findByTestId("run-attribution-panel")).toHaveTextContent("42s");
    expect(screen.getByTestId("run-attribution-panel")).toHaveTextContent("Role default");
  });

  it("retries the shared query when an unavailable attribution is expanded again", async () => {
    const retryAttribution = vi.fn();
    const user = userEvent.setup();
    render(<RunAttributionWidget runId="run-2" startedAt="2026-07-31T00:00:00Z" completedAt="2026-07-31T00:00:01Z" isAttributionError retryAttribution={retryAttribution} />);

    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(screen.getByText("Run attribution is unavailable.")).toBeInTheDocument();
    await user.click(screen.getByTestId("run-attribution-toggle"));
    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(retryAttribution).toHaveBeenCalledTimes(2);
  });

  it("keeps the message-derived duration and loading state while attribution is pending", async () => {
    const user = userEvent.setup();
    render(<RunAttributionWidget
      runId="run-pending"
      startedAt="2026-07-31T00:00:00Z"
      completedAt="2026-07-31T00:00:09Z"
      isAttributionPending
    />);

    expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Agent worked for 9s");
    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(screen.getByTestId("run-attribution-loading")).toBeInTheDocument();
    expect(screen.queryByText("Run attribution is unavailable.")).not.toBeInTheDocument();
  });

  it("honestly degrades null Codex cost, cache write, and role data", async () => {
    const user = userEvent.setup();
    render(<RunAttributionWidget runId="run-3" startedAt="2026-07-31T00:00:00Z" completedAt="2026-07-31T00:00:42Z" attribution={attribution({
      cacheCreationTokens: null,
      estimatedUsd: null,
      launchRole: null,
    })} />);

    expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Agent worked for 42s");
    await user.click(screen.getByTestId("run-attribution-toggle"));
    const panel = await screen.findByTestId("run-attribution-panel");
    expect(panel).toHaveTextContent("Tokens");
    expect(panel).not.toHaveTextContent("cache write");
    expect(panel).not.toHaveTextContent("Est. cost");
  });
});
