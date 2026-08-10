import { createElement, type ReactNode } from "react";

import { QueryClientProvider, useMutation } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import {
  getQueryClient,
  removeQueryClient,
  resetQueryClient,
} from "./queryClient";

function remoteError(
  code: ConstructorParameters<typeof RemoteTransportError>[0]["code"],
): RemoteTransportError {
  return new RemoteTransportError({
    code,
    message: code,
    environmentId: "remote-1",
  });
}

async function executeFailingMutation(
  queryClient: ReturnType<typeof getQueryClient>,
  error: Error,
  meta?: { readonly unknownOutcomeQueryKeys: readonly (readonly unknown[])[] },
): Promise<ReturnType<typeof vi.fn>> {
  const mutationFn = vi.fn().mockRejectedValue(error);
  const mutation = queryClient.getMutationCache().build(queryClient, {
    mutationFn,
    ...(meta !== undefined && { meta }),
  });

  await expect(mutation.execute(undefined)).rejects.toBe(error);
  return mutationFn;
}

afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
});

describe("getQueryClient", () => {
  it("retains one isolated client and cache per environment", () => {
    const environmentA = getQueryClient("env-a");
    const environmentAAgain = getQueryClient("env-a");
    const environmentB = getQueryClient("env-b");

    environmentA.setQueryData(["shared-key"], "environment-a-data");

    expect(environmentAAgain).toBe(environmentA);
    expect(environmentB).not.toBe(environmentA);
    expect(environmentA.getQueryData(["shared-key"])).toBe(
      "environment-a-data",
    );
    expect(environmentB.getQueryData(["shared-key"])).toBeUndefined();
  });

  it("uses the transport environment for its default argument", () => {
    const localClient = getQueryClient();

    setTransportEnvironmentId("env-b");

    expect(getQueryClient()).toBe(getQueryClient("env-b"));
    expect(getQueryClient()).not.toBe(localClient);
  });

  it("drops every retained client when reset", () => {
    const environmentA = getQueryClient("env-a");
    const environmentB = getQueryClient("env-b");

    resetQueryClient();

    expect(getQueryClient("env-a")).not.toBe(environmentA);
    expect(getQueryClient("env-b")).not.toBe(environmentB);
  });

  it("clears and forgets only the removed environment client", () => {
    const environmentA = getQueryClient("env-a");
    const environmentB = getQueryClient("env-b");
    environmentA.setQueryData(["key"], "a");
    environmentB.setQueryData(["key"], "b");

    removeQueryClient("env-a");

    expect(environmentA.getQueryData(["key"])).toBeUndefined();
    expect(getQueryClient("env-a")).not.toBe(environmentA);
    expect(getQueryClient("env-b")).toBe(environmentB);
  });
});

describe("unknown-outcome reconciliation", () => {
  it("invalidates all queries when an unknown outcome has no metadata", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");
    queryClient.setQueryData(["projects"], "projects");

    await executeFailingMutation(
      queryClient,
      remoteError("REMOTE_TIMEOUT_UNKNOWN"),
    );

    expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(["projects"])?.isInvalidated).toBe(true);
  });

  it("invalidates only declared query keys for an unknown outcome", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");
    queryClient.setQueryData(["projects"], "projects");

    await executeFailingMutation(
      queryClient,
      remoteError("REMOTE_REQUEST_IN_PROGRESS"),
      { unknownOutcomeQueryKeys: [["tasks"]] },
    );

    expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(["projects"])?.isInvalidated).toBe(false);
  });

  it("does not invalidate queries for a plain error", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");

    await executeFailingMutation(queryClient, new Error("command failed"));

    expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(false);
  });

  it("does not invalidate queries for a known transport failure", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");

    await executeFailingMutation(queryClient, remoteError("REMOTE_FORBIDDEN"));

    expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(false);
  });

  it("does not resend a mutation after an unknown outcome", async () => {
    const queryClient = getQueryClient("remote-1");

    const mutationFn = await executeFailingMutation(
      queryClient,
      remoteError("REMOTE_TIMEOUT_UNKNOWN"),
    );

    expect(mutationFn).toHaveBeenCalledTimes(1);
  });

  it("invalidates only the client where the unknown outcome occurred", async () => {
    const localClient = getQueryClient("local");
    const remoteClient = getQueryClient("remote-1");
    localClient.setQueryData(["tasks"], "local tasks");
    remoteClient.setQueryData(["tasks"], "remote tasks");

    await executeFailingMutation(
      remoteClient,
      remoteError("REMOTE_TIMEOUT_UNKNOWN"),
    );

    expect(remoteClient.getQueryState(["tasks"])?.isInvalidated).toBe(true);
    expect(localClient.getQueryState(["tasks"])?.isInvalidated).toBe(false);
  });
  // The tests above build mutations straight off the cache. These two go through the
  // production entry point instead — `useMutation` under a provider — so a regression in
  // how `meta` reaches `mutation.options.meta` cannot pass unnoticed.
  it("reconciles an unknown outcome raised through useMutation", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(
      () =>
        useMutation({
          mutationFn: () =>
            Promise.reject(remoteError("REMOTE_TIMEOUT_UNKNOWN")),
        }),
      { wrapper },
    );
    result.current.mutate(undefined);

    await waitFor(() =>
      expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(true),
    );
  });

  it("honours useMutation meta when narrowing an unknown outcome", async () => {
    const queryClient = getQueryClient("remote-1");
    queryClient.setQueryData(["tasks"], "tasks");
    queryClient.setQueryData(["projects"], "projects");
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(
      () =>
        useMutation({
          mutationFn: () =>
            Promise.reject(remoteError("REMOTE_TIMEOUT_UNKNOWN")),
          meta: { unknownOutcomeQueryKeys: [["tasks"]] },
        }),
      { wrapper },
    );
    result.current.mutate(undefined);

    await waitFor(() =>
      expect(queryClient.getQueryState(["tasks"])?.isInvalidated).toBe(true),
    );
    expect(queryClient.getQueryState(["projects"])?.isInvalidated).toBe(false);
  });
});
