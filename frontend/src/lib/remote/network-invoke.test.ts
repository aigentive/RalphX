/**
 * NetworkInvoke: dispatch shape, requestId minting, the 8-code taxonomy, Tauri
 * reject-parity, and the two properties that keep a lost mutation from double-applying
 * — no retries (A-5) and unknown-outcome classification (§3.3).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import {
  networkInvoke,
  mintRequestId,
  REMOTE_INVOKE_TIMEOUT_MS,
} from "./network-invoke";
import {
  REMOTE_TRANSPORT_ERROR_CODES,
  RemoteTransportError,
  isUnknownOutcomeCode,
  type RemoteTransportErrorCode,
} from "./transport-errors";

import { resetRequestPacingForTest } from "./request-pacing";

const ENV = "env-uuid-1";

function invokeInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  primitiveInvoke.mockResolvedValue({ outcome: "ok", result: null });
});

afterEach(() => {
  // Never-settling dispatch mocks hold pacer slots past their test; drop them.
  resetRequestPacingForTest();
  vi.useRealTimers();
});

describe("dispatch", () => {
  it("sends {id, requestId, cmd, args} under the `input` struct key", async () => {
    await networkInvoke(ENV, "list_tasks", { projectId: "p1" });

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");
    const input = invokeInput();
    expect(input.id).toBe(ENV);
    expect(input.cmd).toBe("list_tasks");
    expect(input.args).toEqual({ projectId: "p1" });
  });

  it("mints a fresh requestId per call", async () => {
    await networkInvoke(ENV, "list_tasks", {});
    await networkInvoke(ENV, "list_tasks", {});

    const first = invokeInput(0).requestId;
    const second = invokeInput(1).requestId;
    expect(typeof first).toBe("string");
    expect(first).not.toBe(second);
  });

  it("mints ids that are unique across many calls", () => {
    const ids = new Set(Array.from({ length: 500 }, () => mintRequestId()));
    expect(ids.size).toBe(500);
  });

  it("resolves with the command result from the ok envelope", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: [{ id: "task-1" }],
    });
    await expect(networkInvoke(ENV, "list_tasks", {})).resolves.toEqual([
      { id: "task-1" },
    ]);
  });

  it("refuses binary payloads, which have their own endpoint", async () => {
    const error = await networkInvoke(ENV, "upload", new Uint8Array([1, 2])).catch(
      (reason: unknown) => reason
    );
    expect(error).toBeInstanceOf(RemoteTransportError);
    expect((error as RemoteTransportError).code).toBe("REMOTE_COMMAND_UNAVAILABLE");
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });
});

describe("Tauri reject parity (§6.3)", () => {
  it("rejects with the command's own error VALUE, not a transport error", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: { code: "TASK_NOT_FOUND", detail: "gone" },
    });

    const error = await networkInvoke(ENV, "get_task", { id: "x" }).catch(
      (reason: unknown) => reason
    );

    expect(error).toEqual({ code: "TASK_NOT_FOUND", detail: "gone" });
    expect(error).not.toBeInstanceOf(RemoteTransportError);
  });

  it("rejects with a string error as a string, exactly like local IPC", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "Task not found",
    });
    await expect(networkInvoke(ENV, "get_task", { id: "x" })).rejects.toBe(
      "Task not found"
    );
  });

  it("passes a non-taxonomy proxy rejection through untouched", async () => {
    primitiveInvoke.mockRejectedValue("PAIRING_REJECTED: code expired");
    await expect(networkInvoke(ENV, "list_tasks", {})).rejects.toBe(
      "PAIRING_REJECTED: code expired"
    );
  });
});

describe("the 8-code taxonomy", () => {
  it.each(REMOTE_TRANSPORT_ERROR_CODES)("maps %s from the proxy", async (code) => {
    primitiveInvoke.mockRejectedValue(`${code}: host said no`);

    const error = await networkInvoke(ENV, "list_tasks", {}).catch(
      (reason: unknown) => reason
    );

    expect(error).toBeInstanceOf(RemoteTransportError);
    const transport = error as RemoteTransportError;
    expect(transport.code).toBe(code);
    expect(transport.message).toBe("host said no");
    expect(transport.cmd).toBe("list_tasks");
    expect(transport.environmentId).toBe(ENV);
    expect(transport.requestId).toBe(invokeInput().requestId);
  });

  it("maps every code — none is dropped", () => {
    expect(REMOTE_TRANSPORT_ERROR_CODES).toHaveLength(10);
  });

  it.each([
    ["NOT_CONNECTED", "REMOTE_UNREACHABLE"],
    ["SECRET_STORE_UNAVAILABLE", "REMOTE_UNAUTHORIZED"],
    ["DATABASE_ERROR", "REMOTE_UNREACHABLE"],
  ])("maps the pre-taxonomy proxy code %s to %s", async (raw, expected) => {
    primitiveInvoke.mockRejectedValue(`${raw}: proxy refused`);

    const error = await networkInvoke(ENV, "list_tasks", {}).catch(
      (reason: unknown) => reason
    );

    expect((error as RemoteTransportError).code).toBe(expected);
  });

  it("treats an unparseable envelope as a version mismatch, not a silent success", async () => {
    primitiveInvoke.mockResolvedValue({ unexpected: true });

    const error = await networkInvoke(ENV, "list_tasks", {}).catch(
      (reason: unknown) => reason
    );

    expect((error as RemoteTransportError).code).toBe("REMOTE_VERSION_MISMATCH");
  });
});

describe("unknown outcomes are reconciled, never re-sent (§3.3, A-5)", () => {
  it("surfaces REMOTE_TIMEOUT_UNKNOWN after 30 s and does not re-send", async () => {
    vi.useFakeTimers();
    // A dispatch that never settles: the host may or may not have applied it.
    primitiveInvoke.mockReturnValue(new Promise<never>(() => {}));

    const pending = networkInvoke(ENV, "send_agent_message", { text: "hi" }).catch(
      (reason: unknown) => reason
    );
    await vi.advanceTimersByTimeAsync(REMOTE_INVOKE_TIMEOUT_MS);
    const error = await pending;

    expect(error).toBeInstanceOf(RemoteTransportError);
    expect((error as RemoteTransportError).code).toBe("REMOTE_TIMEOUT_UNKNOWN");
    expect((error as RemoteTransportError).isUnknownOutcome).toBe(true);
    expect(
      primitiveInvoke,
      "a timed-out mutation must never be sent a second time"
    ).toHaveBeenCalledTimes(1);
  });

  it("does not time out early", async () => {
    vi.useFakeTimers();
    let settle: ((value: unknown) => void) | undefined;
    primitiveInvoke.mockReturnValue(
      new Promise((resolve) => {
        settle = resolve;
      })
    );

    const pending = networkInvoke(ENV, "list_tasks", {});
    await vi.advanceTimersByTimeAsync(REMOTE_INVOKE_TIMEOUT_MS - 1);
    settle?.({ outcome: "ok", result: "late but fine" });

    await expect(pending).resolves.toBe("late but fine");
  });

  it("classifies REMOTE_REQUEST_IN_PROGRESS as the SAME unknown outcome", async () => {
    primitiveInvoke.mockRejectedValue(
      "REMOTE_REQUEST_IN_PROGRESS: already dispatching"
    );

    const error = await networkInvoke(ENV, "send_agent_message", {}).catch(
      (reason: unknown) => reason
    );

    expect((error as RemoteTransportError).isUnknownOutcome).toBe(true);
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
  });

  it("classifies exactly the two unknown-outcome codes and no others", () => {
    const unknown = REMOTE_TRANSPORT_ERROR_CODES.filter(
      (code: RemoteTransportErrorCode) => isUnknownOutcomeCode(code)
    );
    expect(unknown).toEqual([
      "REMOTE_TIMEOUT_UNKNOWN",
      "REMOTE_REQUEST_IN_PROGRESS",
    ]);
  });

  it("never retries a transport failure", async () => {
    primitiveInvoke.mockRejectedValue("REMOTE_UNREACHABLE: connection refused");
    await networkInvoke(ENV, "list_tasks", {}).catch(() => undefined);
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
  });
});
