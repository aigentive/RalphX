import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PermissionDialog } from "./PermissionDialog";
import type { PermissionRequest } from "@/types/permission";
import { requestPendingGateReconcile } from "@/lib/remote/pending-gate-reconcile";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { UNKNOWN_OUTCOME_MESSAGE } from "@/lib/remote/unknown-outcome";
import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";

// ============================================================================
// Mocks
// ============================================================================

const mockSubscribe = vi.fn();

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: mockSubscribe,
    emit: vi.fn(),
  }),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    permission: {
      resolveRequest: vi.fn(),
      getPendingPermissions: vi.fn(),
      listPendingPermissionGates: vi.fn(),
    },
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/stores/taskStore", () => ({
  useTaskStore: vi.fn((selector: (state: { tasks: Record<string, { title: string }> }) => unknown) =>
    selector({ tasks: { "task-abc": { title: "My Task" } } })
  ),
}));

import { api } from "@/lib/tauri";
import { toast } from "sonner";

const mockResolveRequest = vi.mocked(api.permission.resolveRequest);
const mockListPendingGates = vi.mocked(api.permission.listPendingPermissionGates);
const mockToastError = vi.mocked(toast.error);
const mockToastInfo = vi.mocked(toast.info);

// ============================================================================
// Test factory
// ============================================================================

/** Code quality #6: factory for PermissionRequest test fixtures */
function makeRequest(overrides: Partial<PermissionRequest> = {}): PermissionRequest {
  return {
    request_id: "test-123",
    tool_name: "Bash",
    tool_input: { command: "ls -la" },
    ...overrides,
  };
}

// ============================================================================
// Event capture helpers
// ============================================================================

describe("PermissionDialog", () => {
  // Captured event callbacks keyed by event name
  let eventCallbacks: Record<string, ((payload: unknown) => void)[]> = {};
  let unlistenFn: ReturnType<typeof vi.fn>;

  function emitEvent(eventName: string, payload: unknown) {
    for (const cb of eventCallbacks[eventName] ?? []) {
      cb(payload);
    }
  }

  beforeEach(() => {
    eventCallbacks = {};
    unlistenFn = vi.fn();

    // Code quality #7: removed dead "permission-request" hyphen branch
    mockSubscribe.mockImplementation((eventName: string, callback: (payload: unknown) => void) => {
      if (!eventCallbacks[eventName]) eventCallbacks[eventName] = [];
      eventCallbacks[eventName].push(callback);
      return unlistenFn;
    });

    mockResolveRequest.mockResolvedValue(undefined);
    // Default: no pending permissions on mount
    mockListPendingGates.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.clearAllMocks();
    eventCallbacks = {};
  });

  // ============================================================================
  // Basic rendering
  // ============================================================================

  it("renders nothing when no requests", async () => {
    const { container } = render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); }); // flush hydration
    expect(container).toBeEmptyDOMElement();
  });

  it("listens to permission:request and permission:expired events on mount", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });
    expect(mockSubscribe).toHaveBeenCalledWith("permission:request", expect.any(Function));
    expect(mockSubscribe).toHaveBeenCalledWith("permission:expired", expect.any(Function));
  });

  it("shows dialog when permission request received", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); }); // flush hydration

    emitEvent("permission:request", makeRequest());

    await waitFor(() => {
      expect(screen.getByText("Permission Required")).toBeInTheDocument();
    });

    expect(screen.getByText("Bash")).toBeInTheDocument();
    expect(screen.getByText("ls -la")).toBeInTheDocument();
  });

  it("displays tool name and formatted input for Bash", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ tool_input: { command: "echo hello" } }));

    await waitFor(() => {
      expect(screen.getByText("echo hello")).toBeInTheDocument();
    });
  });

  it("displays formatted input for Write tool", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Write",
      tool_input: { file_path: "/tmp/test.txt", content: "Hello world!" },
    }));

    await waitFor(() => {
      expect(screen.getByText(/Write to: \/tmp\/test.txt/)).toBeInTheDocument();
      expect(screen.getByText(/Hello world!/)).toBeInTheDocument();
    });
  });

  it("displays formatted input for Write tool when Claude sends camelCase filePath", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Write",
      tool_input: { filePath: "/tmp/test.txt", content: "Hello from camelCase!" },
    }));

    await waitFor(() => {
      expect(screen.getByText(/Write to: \/tmp\/test.txt/)).toBeInTheDocument();
      expect(screen.getByText(/Hello from camelCase!/)).toBeInTheDocument();
    });
  });

  it("truncates long Write content", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Write",
      tool_input: { file_path: "/tmp/test.txt", content: "a".repeat(300) },
    }));

    await waitFor(() => {
      expect(screen.getByText(/\.\.\./)).toBeInTheDocument();
    });
  });

  it("displays formatted input for Edit tool", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Edit",
      tool_input: { file_path: "/tmp/test.txt", old_string: "old value", new_string: "new value" },
    }));

    await waitFor(() => {
      expect(screen.getByText(/Edit: \/tmp\/test.txt/)).toBeInTheDocument();
      expect(screen.getByText(/old value/)).toBeInTheDocument();
      expect(screen.getByText(/new value/)).toBeInTheDocument();
    });
  });

  it("displays formatted input for Read tool", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Read",
      tool_input: { file_path: "/tmp/test.txt" },
    }));

    await waitFor(() => {
      expect(screen.getByText("Read: /tmp/test.txt")).toBeInTheDocument();
    });
  });

  it("displays formatted input for Read tool when Claude sends path", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({
      tool_name: "Read",
      tool_input: { path: "/tmp/test.txt" },
    }));

    await waitFor(() => {
      expect(screen.getByText("Read: /tmp/test.txt")).toBeInTheDocument();
    });
  });

  it("displays context when provided", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ context: "Listing directory contents" }));

    await waitFor(() => {
      expect(screen.getByText("Listing directory contents")).toBeInTheDocument();
    });
  });

  it("shows a short hint explaining that Allow lets the agent continue", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => {
      expect(
        screen.getByText(/Allow approves this exact request and lets the agent continue/i)
      ).toBeInTheDocument();
    });
  });

  it("shows queue count when multiple requests pending", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ request_id: "test-1", tool_input: { command: "ls" } }));
    emitEvent("permission:request", makeRequest({ request_id: "test-2", tool_name: "Read", tool_input: { file_path: "/tmp/test.txt" } }));

    await waitFor(() => {
      expect(screen.getByText("+1 more permission request(s) waiting")).toBeInTheDocument();
    });
  });

  // ============================================================================
  // Decision handling
  // ============================================================================

  it("calls resolve_permission_request with allow on Allow button click", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    expect(mockResolveRequest).toHaveBeenCalledWith({
      requestId: "test-123",
      decision: "allow",
    });
  });

  it("calls resolve_permission_request with deny on Deny button click", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Deny")).toBeInTheDocument(); });
    await user.click(screen.getByText("Deny"));

    expect(mockResolveRequest).toHaveBeenCalledWith({
      requestId: "test-123",
      decision: "deny",
      message: "User denied permission",
    });
  });

  it("removes request from queue after decision", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(screen.queryByText("Permission Required")).not.toBeInTheDocument();
    });
  });

  it("shows next request after resolving first", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ request_id: "test-1", tool_input: { command: "ls" } }));
    emitEvent("permission:request", makeRequest({ request_id: "test-2", tool_name: "Read", tool_input: { file_path: "/tmp/test.txt" } }));

    await waitFor(() => { expect(screen.getByText("ls")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(screen.getByText("Read: /tmp/test.txt")).toBeInTheDocument();
    });
  });

  it("treats dialog close as deny", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByTestId("dialog-close")).toBeInTheDocument(); });
    await user.click(screen.getByTestId("dialog-close"));

    expect(mockResolveRequest).toHaveBeenCalledWith({
      requestId: "test-123",
      decision: "deny",
      message: "User denied permission",
    });
  });

  it("cleans up event listener on unmount", async () => {
    const { unmount } = render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    await waitFor(() => { expect(mockSubscribe).toHaveBeenCalled(); });
    unmount();

    await waitFor(() => { expect(unlistenFn).toHaveBeenCalled(); });
  });

  // ============================================================================
  // Identity UI tests
  // ============================================================================

  it("shows identity row when agent_type provided", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ agent_type: "ralphx-execution-worker" }));

    await waitFor(() => { expect(screen.getByText("Worker")).toBeInTheDocument(); });
  });

  it("shows context label when context_type provided", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ context_type: "task_execution" }));

    await waitFor(() => { expect(screen.getByText("Executing")).toBeInTheDocument(); });
  });

  it("hides identity row when no identity fields", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Permission Required")).toBeInTheDocument(); });

    expect(screen.queryByText("Worker")).not.toBeInTheDocument();
    expect(screen.queryByText("Executing")).not.toBeInTheDocument();
    expect(screen.queryByText(/^Task:/)).not.toBeInTheDocument();
  });

  // ============================================================================
  // resolvingId guards (D8)
  // ============================================================================

  it("buttons disabled while resolving", async () => {
    mockResolveRequest.mockImplementation(() => new Promise(() => {}));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(screen.getByText("Allow").closest("button")).toBeDisabled();
      expect(screen.getByText("Deny").closest("button")).toBeDisabled();
      expect(screen.getByText("Hide").closest("button")).toBeDisabled();
    });
  });

  it("dialog close guard blocks close when resolving", async () => {
    mockResolveRequest.mockImplementation(() => new Promise(() => {}));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(screen.getByText("Allow").closest("button")).toBeDisabled();
    });

    expect(mockResolveRequest).toHaveBeenCalledTimes(1);

    const closeButton = screen.queryByTestId("dialog-close");
    if (closeButton) {
      await user.click(closeButton);
      // Still only called once — the close guard blocked it
      expect(mockResolveRequest).toHaveBeenCalledTimes(1);
    }
  });

  // ============================================================================
  // Smart error handling (D4)
  // ============================================================================

  it("transport error on resolve keeps request in queue and shows retry toast", async () => {
    mockResolveRequest.mockRejectedValue(new Error("Network error"));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(mockToastError).toHaveBeenCalledWith("Failed to resolve permission request, please retry");
    });

    // Dialog still visible — request kept in queue for retry
    expect(screen.getByText("Permission Required")).toBeInTheDocument();
  });

  it("'not found' error removes request from queue and shows expired toast", async () => {
    mockResolveRequest.mockRejectedValue(new Error("Permission request 'test-123' not found"));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(mockToastInfo).toHaveBeenCalledWith("Permission request expired");
    });

    // Dialog dismissed — request removed from queue
    await waitFor(() => {
      expect(screen.queryByText("Permission Required")).not.toBeInTheDocument();
    });
  });

  it("keeps a remote not-found transport failure queued and renders it inline", async () => {
    mockResolveRequest.mockRejectedValue(
      new RemoteTransportError({
        code: "REMOTE_UNREACHABLE",
        message: "remote environment not found: studio-mac",
        environmentId: "studio-mac",
      })
    );

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });
    emitEvent("permission:request", makeRequest());

    await user.click(await screen.findByText("Allow"));

    expect(await screen.findByTestId("permission-remote-error")).toBeInTheDocument();
    expect(screen.getByText("Permission Required")).toBeInTheDocument();
    expect(mockToastInfo).not.toHaveBeenCalledWith("Permission request expired");
    expect(mockToastError).not.toHaveBeenCalled();
  });

  it("non-Error thrown object is normalized correctly in error handling", async () => {
    // Simulate Tauri throwing a plain string (not an Error object)
    mockResolveRequest.mockRejectedValue("Permission request 'test-123' not found");

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());

    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });
    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      // String(error) should yield the raw string, which includes 'not found'
      expect(mockToastInfo).toHaveBeenCalledWith("Permission request expired");
    });
  });

  // ============================================================================
  // permission:expired event (D9)
  // ============================================================================

  it("permission:expired event removes request from queue", async () => {
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());
    await waitFor(() => { expect(screen.getByText("Permission Required")).toBeInTheDocument(); });

    await act(async () => {
      emitEvent("permission:expired", { request_id: "test-123" });
      // Flush the setTimeout(..., 0) from D9
      await new Promise((r) => setTimeout(r, 0));
    });

    await waitFor(() => {
      expect(screen.queryByText("Permission Required")).not.toBeInTheDocument();
    });
    expect(mockToastInfo).toHaveBeenCalledWith("Permission request timed out");
  });

  it("expiry during active resolve for same request skips toast (D8 race guard)", async () => {
    // Infinite promise — resolve never completes
    mockResolveRequest.mockImplementation(() => new Promise(() => {}));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());
    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });

    // Start resolving (sets resolvingId = "test-123")
    await user.click(screen.getByText("Allow"));
    await waitFor(() => {
      expect(screen.getByText("Allow").closest("button")).toBeDisabled();
    });

    // Expiry fires for the same request while it's being resolved
    await act(async () => {
      emitEvent("permission:expired", { request_id: "test-123" });
      await new Promise((r) => setTimeout(r, 0));
    });

    // Toast should NOT have been shown (resolve handler will catch "not found")
    expect(mockToastInfo).not.toHaveBeenCalledWith("Permission request timed out");
  });

  it("expiry for different request shows toast (D8 multi-request guard)", async () => {
    mockResolveRequest.mockImplementation(() => new Promise(() => {}));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest({ request_id: "req-1" }));
    emitEvent("permission:request", makeRequest({ request_id: "req-2" }));
    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });

    // Start resolving req-1
    await user.click(screen.getByText("Allow"));
    await waitFor(() => {
      expect(screen.getByText("Allow").closest("button")).toBeDisabled();
    });

    // Expiry for req-2 (the waiting one) — should show toast
    await act(async () => {
      emitEvent("permission:expired", { request_id: "req-2" });
      await new Promise((r) => setTimeout(r, 0));
    });

    expect(mockToastInfo).toHaveBeenCalledWith("Permission request timed out");
  });

  // ============================================================================
  // Hide button (D6)
  // ============================================================================

  it("hide button removes request from queue without backend call", async () => {
    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());
    await waitFor(() => { expect(screen.getByText("Hide")).toBeInTheDocument(); });

    await user.click(screen.getByText("Hide"));

    await waitFor(() => {
      expect(screen.queryByText("Permission Required")).not.toBeInTheDocument();
    });
    expect(mockResolveRequest).not.toHaveBeenCalled();
    expect(mockToastInfo).toHaveBeenCalledWith("Permission request hidden");
  });

  it("hide button disabled while resolving", async () => {
    mockResolveRequest.mockImplementation(() => new Promise(() => {}));

    const user = userEvent.setup();
    render(<PermissionDialog />);
    await act(async () => { await Promise.resolve(); });

    emitEvent("permission:request", makeRequest());
    await waitFor(() => { expect(screen.getByText("Allow")).toBeInTheDocument(); });

    await user.click(screen.getByText("Allow"));

    await waitFor(() => {
      expect(screen.getByText("Hide").closest("button")).toBeDisabled();
    });
  });

  // ============================================================================
  // Hydration (D7)
  // ============================================================================

  it("hydration seeds queue on mount from the registered pending-gates read", async () => {
    const pending = [makeRequest({ request_id: "hydrated-1" })];
    mockListPendingGates.mockResolvedValue(pending);

    render(<PermissionDialog />);

    await act(async () => { await Promise.resolve(); });

    await waitFor(() => {
      expect(screen.getByText("Permission Required")).toBeInTheDocument();
    });
    expect(mockListPendingGates).toHaveBeenCalledOnce();
  });

  it("hydration deduplicates: request arriving via event before hydration resolves is not shown twice", async () => {
    let resolveHydration!: (value: PermissionRequest[]) => void;
    mockListPendingGates.mockImplementation(
      () => new Promise<PermissionRequest[]>((r) => { resolveHydration = r; })
    );

    const request = makeRequest({ request_id: "dup-req" });

    render(<PermissionDialog />);

    // Event arrives while hydration is still pending (buffered)
    emitEvent("permission:request", request);

    // Hydration resolves with same request
    await act(async () => {
      resolveHydration([request]);
      await Promise.resolve();
    });

    // Only one entry in queue (no duplicate)
    await waitFor(() => {
      expect(screen.getByText("Permission Required")).toBeInTheDocument();
    });
    expect(screen.queryByText("+0 more permission request(s) waiting")).not.toBeInTheDocument();
    expect(screen.queryByText("+1 more permission request(s) waiting")).not.toBeInTheDocument();
  });

  it("preserves a buffered permission request when strict hydration fails", async () => {
    let rejectHydration!: (reason: unknown) => void;
    mockListPendingGates.mockImplementation(
      () => new Promise<PermissionRequest[]>((_, reject) => { rejectHydration = reject; })
    );
    render(<PermissionDialog />);
    emitEvent("permission:request", makeRequest({
      request_id: "buffered-after-read-error",
      tool_input: { command: "keep buffered" },
    }));

    await act(async () => {
      rejectHydration(new Error("pending read failed"));
      await Promise.resolve();
    });

    expect(screen.getByText("keep buffered")).toBeInTheDocument();
  });

  it("hydration race guard buffers permission:expired during hydration and replays — skips toast for pre-expired requests", async () => {
    // Hydration returns req-known (snapshot), but req-gone was already expired before snapshot
    const req = makeRequest({ request_id: "req-known" });
    let resolveHydration!: (value: PermissionRequest[]) => void;
    mockListPendingGates.mockImplementation(
      () => new Promise<PermissionRequest[]>((r) => { resolveHydration = r; })
    );

    render(<PermissionDialog />);

    // Two expiry events arrive while hydrating:
    // - "req-gone": was NOT in the hydration snapshot → should NOT show toast
    // - "req-known": WAS in the hydration snapshot → should show toast
    emitEvent("permission:expired", { request_id: "req-gone" });
    emitEvent("permission:expired", { request_id: "req-known" });

    // Hydration resolves with only req-known in the snapshot
    await act(async () => {
      resolveHydration([req]);
      await Promise.resolve();
    });

    // req-gone toast should NOT have been shown (not in snapshot)
    // req-known toast SHOULD have been shown (was in snapshot)
    expect(mockToastInfo).toHaveBeenCalledTimes(1);
    expect(mockToastInfo).toHaveBeenCalledWith("Permission request timed out");
  });

  it("re-opens from a notification by merging pending requests and moving the selected request first", async () => {
    const user = userEvent.setup();
    const selected = makeRequest({ request_id: "from-notification", tool_input: { command: "selected command" } });
    const pending = makeRequest({ request_id: "from-backend", tool_name: "Read", tool_input: { file_path: "/tmp/pending" } });
    mockListPendingGates.mockResolvedValueOnce([]).mockResolvedValueOnce([pending, selected]);

    render(<PermissionDialog />);
    await waitFor(() => expect(mockListPendingGates).toHaveBeenCalledOnce());
    emitEvent("permission:request", makeRequest({ request_id: "already-visible", tool_input: { command: "existing command" } }));
    await waitFor(() => expect(screen.getByText("existing command")).toBeInTheDocument());

    await act(async () => {
      window.dispatchEvent(new CustomEvent("ralphx:open-permission-dialog", { detail: { requestId: "from-notification" } }));
      await Promise.resolve();
    });

    await waitFor(() => expect(screen.getByText("selected command")).toBeInTheDocument());
    expect(mockListPendingGates).toHaveBeenCalledTimes(2);
    expect(screen.getByText("+2 more permission request(s) waiting")).toBeInTheDocument();

    await user.click(screen.getByText("Hide"));
    await waitFor(() => expect(screen.getByText("existing command")).toBeInTheDocument());
  });

  // ==========================================================================
  // P-21: authoritative gate reconciliation on (re)connect (PR 2.7-c)
  // ==========================================================================

  describe("pending-gate reconciliation", () => {
    async function reconcile(environmentId = LOCAL_ENVIRONMENT_ID) {
      await act(async () => {
        requestPendingGateReconcile(environmentId);
        await Promise.resolve();
        await Promise.resolve();
      });
    }

    it("adopts a gate raised while this client was disconnected", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      expect(screen.queryByText("Permission Required")).toBeNull();

      mockListPendingGates.mockResolvedValue([
        makeRequest({ request_id: "raised-offline", tool_input: { command: "while away" } }),
      ]);
      await reconcile();

      expect(screen.getByText("while away")).toBeInTheDocument();
    });

    it("drops a gate the host no longer lists — resolved elsewhere, not still waiting", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      emitEvent("permission:request", makeRequest({ request_id: "stale", tool_input: { command: "already answered" } }));
      await waitFor(() => expect(screen.getByText("already answered")).toBeInTheDocument());

      mockListPendingGates.mockResolvedValue([]);
      await reconcile();

      expect(screen.queryByText("already answered")).toBeNull();
    });

    it("FAILS CLOSED on a read error: keeps the prior UI and says so", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      emitEvent("permission:request", makeRequest({ request_id: "live", tool_input: { command: "still pending" } }));
      await waitFor(() => expect(screen.getByText("still pending")).toBeInTheDocument());

      mockListPendingGates.mockRejectedValue(new Error("gate state unreadable"));
      await reconcile();

      // "No data" must never authorize clearing a live permission prompt.
      expect(screen.getByText("still pending")).toBeInTheDocument();
      expect(mockToastError).toHaveBeenCalledWith(
        "Couldn't refresh pending permission requests"
      );
    });

    // Both gate lists ARE on the registered facade surface (3.1 batch 1), so
    // REMOTE_COMMAND_UNAVAILABLE from them is a regression, not a capability gap.
    it("surfaces REMOTE_COMMAND_UNAVAILABLE instead of swallowing it", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      emitEvent("permission:request", makeRequest({ request_id: "live", tool_input: { command: "still pending" } }));
      await waitFor(() => expect(screen.getByText("still pending")).toBeInTheDocument());

      mockListPendingGates.mockRejectedValue(
        new RemoteTransportError({
          code: "REMOTE_COMMAND_UNAVAILABLE",
          message: "not registered on this host",
          environmentId: LOCAL_ENVIRONMENT_ID,
        })
      );
      await reconcile();

      // Fail closed either way: the live request is never cleared by a failed read.
      expect(screen.getByText("still pending")).toBeInTheDocument();
      expect(mockToastError).toHaveBeenCalled();
    });

    it("ignores a BACKGROUND environment's connect (SCOPE negative)", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      mockListPendingGates.mockClear();
      emitEvent("permission:request", makeRequest({ request_id: "live", tool_input: { command: "still pending" } }));
      await waitFor(() => expect(screen.getByText("still pending")).toBeInTheDocument());

      mockListPendingGates.mockResolvedValue([]);
      await reconcile("some-other-environment");

      expect(mockListPendingGates).not.toHaveBeenCalled();
      expect(screen.getByText("still pending")).toBeInTheDocument();
    });

    it("keeps a gate raised WHILE the authoritative list was in flight", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });

      // The list call departs before the gate exists, so its response can never
      // mention it. Dropping it here loses a live prompt no further event re-raises.
      let settleList: ((pending: PermissionRequest[]) => void) | undefined;
      mockListPendingGates.mockImplementation(
        () => new Promise<PermissionRequest[]>((resolve) => { settleList = resolve; })
      );
      await act(async () => {
        requestPendingGateReconcile(LOCAL_ENVIRONMENT_ID);
        await Promise.resolve();
      });

      emitEvent("permission:request", makeRequest({
        request_id: "raised-midflight",
        tool_input: { command: "arrived late" },
      }));
      await waitFor(() => expect(screen.getByText("arrived late")).toBeInTheDocument());

      await act(async () => {
        settleList?.([]);
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.getByText("arrived late")).toBeInTheDocument();
    });

    it("still drops a pre-call gate the host no longer lists", async () => {
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });
      emitEvent("permission:request", makeRequest({
        request_id: "pre-call",
        tool_input: { command: "already answered" },
      }));
      await waitFor(() => expect(screen.getByText("already answered")).toBeInTheDocument());

      let settleList: ((pending: PermissionRequest[]) => void) | undefined;
      mockListPendingGates.mockImplementation(
        () => new Promise<PermissionRequest[]>((resolve) => { settleList = resolve; })
      );
      await act(async () => {
        requestPendingGateReconcile(LOCAL_ENVIRONMENT_ID);
        await Promise.resolve();
      });
      await act(async () => {
        settleList?.([]);
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.queryByText("already answered")).toBeNull();
    });
  });

  // ==========================================================================
  // P-20: unknown mutation outcome — refetch, never re-send (PR 2.7-b)
  // ==========================================================================

  describe("unknown resolve outcome", () => {
    for (const code of ["REMOTE_TIMEOUT_UNKNOWN", "REMOTE_REQUEST_IN_PROGRESS"] as const) {
      it(`reconciles ${code} with exactly one refetch and zero re-sends`, async () => {
        const user = userEvent.setup();
        render(<PermissionDialog />);
        await act(async () => { await Promise.resolve(); });

        emitEvent("permission:request", makeRequest({
          request_id: "maybe-applied",
          tool_input: { command: "maybe applied" },
        }));
        await waitFor(() => expect(screen.getByText("maybe applied")).toBeInTheDocument());

        mockListPendingGates.mockClear();
        // The host already applied it — the refetch is what settles that.
        mockListPendingGates.mockResolvedValue([]);
        mockResolveRequest.mockRejectedValue(
          new RemoteTransportError({
            code,
            message: "no answer",
            environmentId: LOCAL_ENVIRONMENT_ID,
          })
        );

        await user.click(screen.getByText("Allow"));

        await waitFor(() =>
          expect(mockListPendingGates).toHaveBeenCalledTimes(1)
        );
        expect(mockResolveRequest).toHaveBeenCalledTimes(1);
        // "please retry" is the advice that invites the forbidden blind re-send.
        expect(mockToastError).not.toHaveBeenCalled();
        expect(mockToastInfo).toHaveBeenCalledWith(UNKNOWN_OUTCOME_MESSAGE);
        await waitFor(() => expect(screen.queryByText("maybe applied")).toBeNull());
      });
    }

    it("keeps the request when the host still lists it as pending", async () => {
      const user = userEvent.setup();
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });

      const pending = makeRequest({
        request_id: "still-open",
        tool_input: { command: "never landed" },
      });
      emitEvent("permission:request", pending);
      await waitFor(() => expect(screen.getByText("never landed")).toBeInTheDocument());

      mockListPendingGates.mockClear();
      mockListPendingGates.mockResolvedValue([pending]);
      mockResolveRequest.mockRejectedValue(
        new RemoteTransportError({
          code: "REMOTE_TIMEOUT_UNKNOWN",
          message: "no answer",
          environmentId: LOCAL_ENVIRONMENT_ID,
        })
      );

      await user.click(screen.getByText("Allow"));

      await waitFor(() =>
        expect(mockListPendingGates).toHaveBeenCalledTimes(1)
      );
      expect(mockResolveRequest).toHaveBeenCalledTimes(1);
      expect(screen.getByText("never landed")).toBeInTheDocument();
    });

    it("leaves an unreadable refetch as a no-op: the prompt stays up", async () => {
      const user = userEvent.setup();
      render(<PermissionDialog />);
      await act(async () => { await Promise.resolve(); });

      emitEvent("permission:request", makeRequest({
        request_id: "unreadable",
        tool_input: { command: "state unknown" },
      }));
      await waitFor(() => expect(screen.getByText("state unknown")).toBeInTheDocument());

      mockListPendingGates.mockClear();
      mockListPendingGates.mockRejectedValue(new Error("unreadable"));
      mockResolveRequest.mockRejectedValue(
        new RemoteTransportError({
          code: "REMOTE_TIMEOUT_UNKNOWN",
          message: "no answer",
          environmentId: LOCAL_ENVIRONMENT_ID,
        })
      );

      await user.click(screen.getByText("Allow"));

      await waitFor(() =>
        expect(mockListPendingGates).toHaveBeenCalledTimes(1)
      );
      expect(mockResolveRequest).toHaveBeenCalledTimes(1);
      expect(screen.getByText("state unknown")).toBeInTheDocument();
    });
  });
});
