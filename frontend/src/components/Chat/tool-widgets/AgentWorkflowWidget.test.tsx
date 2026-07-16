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
    const run = {
          id: "run-1",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "queued",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:00Z",
          completed_at: null,
          error: null,
        };
    const fetchMock = vi.fn<typeof fetch>(async (input) => {
      const url = String(input);
      if (url.endsWith("agent_workflows/runs/latest")) return jsonResponse(null);
      if (url.endsWith("agent_workflows/scripts/approve")) {
        return jsonResponse({ approved: true });
      }
      if (url.endsWith("agent_workflows/runs/start")) return jsonResponse(run);
      return jsonResponse({
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
        });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    expect(await screen.findByTestId("agent-workflow-approval")).toHaveTextContent(
      "Permission envelope: {\"filesystem\":\"read-only\"}",
    );
    expect(screen.getByText("Estimated fanout: 2")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Run once" }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        backendApiUrl("agent_workflows/scripts/approve"),
        expect.objectContaining({
          body: JSON.stringify({
            script_id: "script-1",
            script_hash: "script-hash",
            permission_hash: "permission-hash",
          }),
        }),
      ),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      backendApiUrl("agent_workflows/runs/start"),
      expect.any(Object),
    );
    const startCall = fetchMock.mock.calls.find(([input]) =>
      String(input).endsWith("agent_workflows/runs/start"),
    );
    expect(
      JSON.parse(String(startCall?.[1]?.body)),
    ).toEqual({
      script_id: "script-1",
      script_hash: "script-hash",
      permission_hash: "permission-hash",
      launch_id: expect.any(String),
      args: {},
    });
    expect(await screen.findByTestId("agent-workflow-progress")).toHaveTextContent(
      "Review",
    );
    expect(screen.getByTestId("agent-workflow-progress")).toHaveTextContent("Tokens: 175");
    expect(screen.getByTestId("agent-workflow-progress")).toHaveTextContent("Cost: $0.0125");
  });

  it("can dismiss an unapproved workflow without launching it", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(null));
    vi.stubGlobal("fetch", fetchMock);
    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    await screen.findByTestId("agent-workflow-approval");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.getByText("Workflow was not approved or started.")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith(
      backendApiUrl("agent_workflows/runs/latest"),
      expect.any(Object),
    );
  });

  it("hydrates a UI-started run from durable script linkage after remount", async () => {
    const fetchMock = vi.fn<typeof fetch>(async (input) => {
      const url = String(input);
      if (url.endsWith("agent_workflows/runs/latest")) {
        return jsonResponse({
          id: "run-existing",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "running",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:01Z",
          completed_at: null,
          error: null,
        });
      }
      return jsonResponse({
        run: {
          id: "run-existing",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "running",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:01Z",
          completed_at: null,
          error: null,
        },
        phases: [],
        invocations: [],
        logs: [],
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
          estimated_usd: 0,
        },
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    expect(await screen.findByTestId("agent-workflow-progress")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run once" })).not.toBeInTheDocument();
  });

  it("keeps an explicit persisted run id ahead of script-level latest lookup", async () => {
    const toolCall = createToolCall();
    toolCall.result = {
      ...(toolCall.result as Record<string, unknown>),
      run: { id: "run-explicit" },
    };
    const requestedRunIds: string[] = [];
    const fetchMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input);
      if (url.endsWith("agent_workflows/runs/latest")) {
        return jsonResponse({
          id: "run-newer",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "running",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:01Z",
          completed_at: null,
          error: null,
        });
      }
      requestedRunIds.push(JSON.parse(String(init?.body)).run_id as string);
      return jsonResponse({
        run: {
          id: "run-explicit",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "completed",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:02Z",
          completed_at: "2026-07-15T00:00:02Z",
          error: null,
        },
        phases: [],
        invocations: [],
        logs: [],
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
          estimated_usd: 0,
        },
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={toolCall} />, { wrapper });

    expect(await screen.findByTestId("agent-workflow-progress")).toBeInTheDocument();
    expect(requestedRunIds).toEqual(["run-explicit"]);
  });

  it("renders disabled runs as read-only and stops progress polling", async () => {
    const fetchMock = vi.fn<typeof fetch>(async (input) => {
      const url = String(input);
      if (url.endsWith("agent_workflows/runs/latest")) {
        return jsonResponse({
          id: "run-disabled",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "disabled",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:05Z",
          completed_at: null,
          error: null,
        });
      }
      return jsonResponse({
        run: {
          id: "run-disabled",
          script_id: "script-1",
          conversation_id: "conversation-1",
          status: "disabled",
          created_at: "2026-07-15T00:00:00Z",
          updated_at: "2026-07-15T00:00:05Z",
          completed_at: null,
          error: null,
        },
        phases: [],
        invocations: [],
        logs: [],
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
          estimated_usd: 0,
        },
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    expect(await screen.findByTestId("agent-workflow-progress")).toHaveTextContent("Elapsed: 5s");
    expect(screen.queryByRole("button", { name: "Pause" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Resume" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();

    await new Promise((resolve) => setTimeout(resolve, 1_100));
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("shows elapsed wall time while a run is active instead of its last update age", async () => {
    const now = Date.now();
    const fetchMock = vi.fn<typeof fetch>(async (input) => {
      const url = String(input);
      const run = {
        id: "run-active",
        script_id: "script-1",
        conversation_id: "conversation-1",
        status: "running",
        created_at: new Date(now - 10_000).toISOString(),
        updated_at: new Date(now - 9_000).toISOString(),
        completed_at: null,
        error: null,
      };
      if (url.endsWith("agent_workflows/runs/latest")) return jsonResponse(run);
      return jsonResponse({
        run,
        phases: [],
        invocations: [],
        logs: [],
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          cache_creation_tokens: 0,
          cache_read_tokens: 0,
          estimated_usd: 0,
        },
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<AgentWorkflowWidget toolCall={createToolCall()} />, { wrapper });

    expect(await screen.findByTestId("agent-workflow-progress")).toHaveTextContent(
      /Elapsed: (?:9|10|11)s/,
    );
  });
});
