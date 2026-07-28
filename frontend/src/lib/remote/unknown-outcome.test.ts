/**
 * P-20 client half (2.7-b). The load-bearing assertions are absences: zero re-sends,
 * zero retries, zero timers, and zero reach into another environment's cache.
 */

import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  reconcileUnknownOutcome,
  UNKNOWN_OUTCOME_MESSAGE,
} from "./unknown-outcome";
import {
  RemoteTransportError,
  type RemoteTransportErrorCode,
} from "./transport-errors";

function transportError(code: RemoteTransportErrorCode): RemoteTransportError {
  return new RemoteTransportError({
    code,
    message: `${code} for update_task`,
    environmentId: "env-a",
    cmd: "update_task",
    requestId: "req-1",
  });
}

let queryClient: QueryClient;

beforeEach(() => {
  vi.useFakeTimers();
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
});

afterEach(() => {
  expect(
    vi.getTimerCount(),
    "reconciliation scheduled a timer; A-5 makes the supervisor the sole retry owner"
  ).toBe(0);
  vi.useRealTimers();
  queryClient.clear();
});

describe("reconcileUnknownOutcome", () => {
  it.each<RemoteTransportErrorCode>([
    "REMOTE_TIMEOUT_UNKNOWN",
    "REMOTE_REQUEST_IN_PROGRESS",
  ])("treats %s as the same unknown outcome: refetch, never re-send", (code) => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    const result = reconcileUnknownOutcome(transportError(code), {
      queryClient,
      queryKeys: [["tasks", "list"], ["tasks", "detail", "t-1"]],
    });

    expect(result).toEqual({
      kind: "reconciled",
      message: UNKNOWN_OUTCOME_MESSAGE,
    });
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenNthCalledWith(1, {
      queryKey: ["tasks", "list"],
    });
    expect(invalidate).toHaveBeenNthCalledWith(2, {
      queryKey: ["tasks", "detail", "t-1"],
    });
  });

  it.each<RemoteTransportErrorCode>([
    "REMOTE_UNAUTHORIZED",
    "REMOTE_FORBIDDEN",
    "REMOTE_UNREACHABLE",
    "REMOTE_COMMAND_UNAVAILABLE",
    "REMOTE_VERSION_MISMATCH",
    "REMOTE_REQUEST_ID_REUSED",
    "REMOTE_INVALID_ARGUMENTS",
    "REMOTE_INTERNAL_ERROR",
  ])("leaves %s to its existing handling and invalidates nothing", (code) => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    const result = reconcileUnknownOutcome(transportError(code), {
      queryClient,
      queryKeys: [["tasks", "list"]],
    });

    expect(result).toEqual({ kind: "not_unknown" });
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("ignores a plain Error rather than guessing it might be unknown", () => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    expect(
      reconcileUnknownOutcome(new Error("boom"), {
        queryClient,
        queryKeys: [["tasks", "list"]],
      })
    ).toEqual({ kind: "not_unknown" });
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("cannot touch another environment's cache (A-8: scoping is the client)", () => {
    const otherEnvironment = new QueryClient();
    const otherInvalidate = vi.spyOn(otherEnvironment, "invalidateQueries");

    reconcileUnknownOutcome(transportError("REMOTE_TIMEOUT_UNKNOWN"), {
      queryClient,
      queryKeys: [["tasks", "list"]],
    });

    expect(otherInvalidate).not.toHaveBeenCalled();
    otherEnvironment.clear();
  });

  it("invalidates nothing when the caller names no entity", () => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    expect(
      reconcileUnknownOutcome(transportError("REMOTE_TIMEOUT_UNKNOWN"), {
        queryClient,
        queryKeys: [],
      })
    ).toEqual({ kind: "reconciled", message: UNKNOWN_OUTCOME_MESSAGE });
    // An empty key list must NOT escalate to a whole-client sweep: the caller said it
    // knows of no affected entity, which is not the same as "refresh everything".
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("runs a non-react-query refetch exactly once", () => {
    const refetch = vi.fn();

    expect(
      reconcileUnknownOutcome(transportError("REMOTE_REQUEST_IN_PROGRESS"), {
        refetch,
      })
    ).toEqual({ kind: "reconciled", message: UNKNOWN_OUTCOME_MESSAGE });
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it("does not refetch for an error it does not own", () => {
    const refetch = vi.fn();

    expect(
      reconcileUnknownOutcome(transportError("REMOTE_FORBIDDEN"), { refetch })
    ).toEqual({ kind: "not_unknown" });
    expect(refetch).not.toHaveBeenCalled();
  });
});
