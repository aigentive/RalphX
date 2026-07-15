import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useTaskValidationEventInvalidation,
  useTaskValidationLiveState,
} from "./useTaskValidationEvents";

const testState = vi.hoisted(() => ({
  subscriptions: new Map<string, Array<(payload: unknown) => void>>(),
  invalidateQueries: vi.fn(),
}));

function fireEvent(event: string, payload: unknown) {
  for (const handler of testState.subscriptions.get(event) ?? []) {
    handler(payload);
  }
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      const handlers = testState.subscriptions.get(event) ?? [];
      handlers.push(handler);
      testState.subscriptions.set(event, handlers);
      return () => {
        testState.subscriptions.set(
          event,
          (testState.subscriptions.get(event) ?? []).filter((candidate) => candidate !== handler),
        );
      };
    },
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: testState.invalidateQueries,
  }),
}));

const TASK_ID = "task-1";
const OTHER_TASK_ID = "task-2";
const RUN_ID = "run-1";
const COMMAND_ID = "command-1";

function validationEvent(overrides: Record<string, unknown> = {}) {
  return {
    type: "run_started",
    task_id: TASK_ID,
    project_id: "project-1",
    run_id: RUN_ID,
    status: "running",
    purpose: "final",
    context_type: "execution",
    mode: "force",
    policy_enabled: true,
    run_started_at: "2026-07-06T11:00:00.000Z",
    emitted_at: "2026-07-06T11:00:00.000Z",
    ...overrides,
  };
}

describe("useTaskValidationLiveState", () => {
  beforeEach(() => {
    testState.subscriptions.clear();
    testState.invalidateQueries.mockClear();
  });

  it("tracks current task run and command output with bounded logs", () => {
    const { result } = renderHook(() => useTaskValidationLiveState(TASK_ID));

    act(() => {
      fireEvent("task_validation:event", validationEvent());
    });

    expect(result.current?.latest_run?.id).toBe(RUN_ID);
    expect(result.current?.commands).toEqual([]);

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_started",
          command_id: COMMAND_ID,
          command_source: "agent_selected",
          command: "npm test",
          cwd: "/repo",
          label: "Unit tests",
          category: "test",
          cache_decision: "ran",
          command_started_at: "2026-07-06T11:00:01.000Z",
        }),
      );
    });

    expect(result.current?.commands[0]).toMatchObject({
      id: COMMAND_ID,
      status: "running",
      command: "npm test",
      label: "Unit tests",
    });

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_output",
          command_id: COMMAND_ID,
          command: "npm test",
          cwd: "/repo",
          category: "test",
          stdout_delta: "a".repeat(4_200),
          stream: "stdout",
        }),
      );
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_output",
          task_id: OTHER_TASK_ID,
          command_id: COMMAND_ID,
          command: "npm test",
          cwd: "/repo",
          category: "test",
          stderr_delta: "wrong task",
          stream: "stderr",
        }),
      );
    });

    expect(result.current?.commands[0].stdout_snippet).toHaveLength(4_000);
    expect(result.current?.commands[0].stdout_snippet).toMatch(/^a+$/);
    expect(result.current?.commands[0].stderr_snippet).toBeNull();

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_completed",
          command_id: COMMAND_ID,
          command: "npm test",
          cwd: "/repo",
          label: "Unit tests",
          category: "test",
          status: "failed",
          cache_decision: "ran",
          exit_code: 1,
          duration_ms: 1234,
          stderr_snippet: "failed",
          command_completed_at: "2026-07-06T11:00:02.000Z",
        }),
      );
    });

    expect(result.current?.commands[0]).toMatchObject({
      status: "failed",
      exit_code: 1,
      duration_ms: 1234,
      stderr_snippet: "failed",
    });
    expect(result.current?.latest_run?.status).toBe("running");

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "run_completed",
          status: "failed",
          run_completed_at: "2026-07-06T11:00:03.000Z",
        }),
      );
    });

    expect(result.current?.latest_run?.status).toBe("failed");
  });
});

describe("useTaskValidationEventInvalidation", () => {
  beforeEach(() => {
    testState.subscriptions.clear();
    testState.invalidateQueries.mockClear();
  });

  it("invalidates summaries for lifecycle events but not output chunks", () => {
    renderHook(() => useTaskValidationEventInvalidation());

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_output",
          command_id: COMMAND_ID,
          command: "npm test",
          cwd: "/repo",
          category: "test",
          stdout_delta: "stream",
          stream: "stdout",
        }),
      );
    });

    expect(testState.invalidateQueries).not.toHaveBeenCalled();

    act(() => {
      fireEvent(
        "task_validation:event",
        validationEvent({
          type: "command_completed",
          command_id: COMMAND_ID,
          command: "npm test",
          cwd: "/repo",
          category: "test",
          status: "passed",
          cache_decision: "ran",
        }),
      );
    });

    expect(testState.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["task-validation", "summary", TASK_ID],
    });
  });
});
