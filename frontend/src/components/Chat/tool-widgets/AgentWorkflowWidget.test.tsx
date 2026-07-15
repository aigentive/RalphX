import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { backendApiUrl } from "@/api/backend";
import { AgentWorkflowWidget } from "./AgentWorkflowWidget";
import type { ToolCall } from "./shared.constants";

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      {children}
    </QueryClientProvider>
  );
}

function jsonResponse(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function createToolCall(): ToolCall {
  return {
    id: "workflow-create-1",
    name: "mcp__ralphx__create_agent_workflow_script",
    arguments: {},
    result: {
      id: "script-1",
      source: 'phase("Review"); return { ok: true };',
      script_hash: "script-hash",
      permission_hash: "permission-hash",
      permission_summary_json: JSON.stringify({ filesystem: "read-only" }),
      estimated_fanout: 2,
      meta: {
        name: "Review migration",
        description: "Cross-check the migration before implementation.",
        phases: ["Review", "Synthesize"],
        maxConcurrency: 2,
        maxInvocations: 4,
      },
    },
  };
}

describe("AgentWorkflowWidget", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reviews exact hashes before launching and then renders durable progress", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse({ approved: true }))
      .mockResolvedValueOnce(
        jsonResponse({
          id: "run-1",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "queued",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:00Z",
          completed_at: null,
          error: null,
        }),
      )
      .mockResolvedValue(
        jsonResponse({
          run: {
            id: "run-1",
            script_id: "script-1",
            conversation_id: "conversation-1",
            status: "running",
            created_at: "2026-07-15T00:00:00Z",
            updated_at: "2026-07-15T00:00:01Z",
            completed_at: null,
            error: null,
          },
          phases: [
            {
              id: "phase-1",
              key: "review",
              name: "Review",
              ordinal: 0,
              status: "running",
              error: null,
            },
          ],
          invocations: [],
          logs: [],
          usage: {
            input_tokens: 100,
            output_tokens: 25,
            cache_creation_tokens: 0,
            cache_read_tokens: 50,
            estimated_usd: 0.0125,
          },
        }),
      );
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    expect(screen.getByTestId("agent-workflow-approval")).toHaveTextContent(
      "Permission envelope: {\"filesystem\":\"read-only\"}",
    );
    expect(screen.getByText("Estimated fanout: 2")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Run once" }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      backendApiUrl("agent_workflows/scripts/approve"),
      expect.objectContaining({
        body: JSON.stringify({
          script_id: "script-1",
          script_hash: "script-hash",
          permission_hash: "permission-hash",
        }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      backendApiUrl("agent_workflows/runs/start"),
      expect.objectContaining({
        body: JSON.stringify({
          script_id: "script-1",
          script_hash: "script-hash",
          permission_hash: "permission-hash",
          args: {},
        }),
      }),
    );
    expect(await screen.findByTestId("agent-workflow-progress")).toHaveTextContent(
      "Review",
    );
    expect(screen.getByTestId("agent-workflow-progress")).toHaveTextContent("Tokens: 175");
    expect(screen.getByTestId("agent-workflow-progress")).toHaveTextContent("Cost: $0.0125");
  });

  it("can dismiss an unapproved workflow without launching it", () => {
    const fetchMock = vi.fn<typeof fetch>();
    vi.stubGlobal("fetch", fetchMock);
    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.getByText("Workflow was not approved or started.")).toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
