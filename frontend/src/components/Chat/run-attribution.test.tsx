import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const getAttribution = vi.hoisted(() => vi.fn());
vi.mock("@/api/agent-runs", () => ({ agentRunsApi: { getAttribution } }));

import { RunAttributionWidget } from "./RunAttributionWidget";
import { roleVerb, workedDurationLabel } from "./run-attribution";

describe("run attribution helpers", () => {
  it("maps feedback roles and formats duration", () => {
    expect(roleVerb("workspace_reviewer")).toBe("Reviewer");
    expect(roleVerb("workspace_repair")).toBe("Fixer");
    expect(roleVerb(null)).toBe("Agent");
    expect(workedDurationLabel("2026-07-31T00:00:00Z", "2026-07-31T00:01:04Z")).toBe("1m 4s");
  });
});

describe("RunAttributionWidget", () => {
  it("is collapsed initially and fetches only once after expansion", async () => {
    getAttribution.mockResolvedValue({ id: "run-1", conversationId: "c", status: "completed", startedAt: "2026-07-31T00:00:00Z", completedAt: "2026-07-31T00:00:42Z", harness: null, upstreamProvider: null, providerProfile: null, providerSessionId: null, logicalModel: null, effectiveModelId: null, logicalEffort: null, effectiveEffort: null, serviceTier: null, approvalPolicy: null, sandboxMode: null, inputTokens: null, outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, estimatedUsd: null, runChainId: null, actionKind: null, personaSlug: null, agentName: "Reviewer", launchRole: "workspace_reviewer", runtimeSource: null });
    const user = userEvent.setup();
    render(<RunAttributionWidget runId="run-1" startedAt="2026-07-31T00:00:00Z" completedAt="2026-07-31T00:00:42Z" launchRole="workspace_reviewer" />);
    expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Reviewer worked for 42s");
    expect(screen.queryByTestId("run-attribution-panel")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("run-attribution-toggle"));
    await screen.findByTestId("run-attribution-panel");
    await user.click(screen.getByTestId("run-attribution-toggle"));
    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(getAttribution).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Provider")).not.toBeInTheDocument();
  });

  it("shows a muted inline error if attribution loading fails", async () => {
    getAttribution.mockRejectedValueOnce(new Error("nope"));
    const user = userEvent.setup();
    render(<RunAttributionWidget runId="run-2" startedAt="2026-07-31T00:00:00Z" completedAt="2026-07-31T00:00:01Z" />);
    await user.click(screen.getByTestId("run-attribution-toggle"));
    expect(await screen.findByText("Run attribution is unavailable.")).toBeInTheDocument();
  });
});
