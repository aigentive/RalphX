/**
 * Transport wrapper: local-passthrough parity (A-3), remote dispatch routing,
 * local-only handling, and the A-13 guard that non-invoke core members keep local
 * semantics.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// `src/test/setup.ts` globally mocks `@tauri-apps/api/core`, which the vitest alias
// points at THIS wrapper. These tests are about the wrapper itself, so put the real
// module back and mock only the primitive underneath it.
vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke:
    vi.fn<(cmd: string, args?: unknown, options?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import * as primitive from "#tauri-core-primitive";
import * as wrapper from "./invoke";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "./active-environment";
import { RemoteTransportError } from "./transport-errors";

const REMOTE_ENV = "env-uuid-1";

beforeEach(() => {
  primitiveInvoke.mockReset();
  primitiveInvoke.mockResolvedValue(undefined);
  resetTransportEnvironmentId();
});

afterEach(() => {
  resetTransportEnvironmentId();
});

describe("local passthrough parity (A-3)", () => {
  it("forwards cmd, args, and options to the primitive unchanged", async () => {
    primitiveInvoke.mockResolvedValue({ id: "task-1" });
    const args = { projectId: "p1", limit: 10 };
    const options = { headers: { "x-test": "1" } };

    const result = await wrapper.invoke("list_tasks", args, options);

    expect(result).toEqual({ id: "task-1" });
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke).toHaveBeenCalledWith("list_tasks", args, options);
    // Same object identity: the wrapper must not clone or normalise args.
    expect(primitiveInvoke.mock.calls[0]?.[1]).toBe(args);
  });

  it("passes an argless call through with undefined, not an empty object", async () => {
    await wrapper.invoke("health_check");
    expect(primitiveInvoke).toHaveBeenCalledWith("health_check", undefined, undefined);
  });

  it("rejects with the primitive's rejection value untouched", async () => {
    const rejection = { code: "TASK_NOT_FOUND", message: "gone" };
    primitiveInvoke.mockRejectedValue(rejection);

    await expect(wrapper.invoke("get_task", { id: "x" })).rejects.toBe(rejection);
  });

  it("passes a string rejection through as a string, matching Tauri", async () => {
    primitiveInvoke.mockRejectedValue("plain failure");
    await expect(wrapper.invoke("get_task", { id: "x" })).rejects.toBe(
      "plain failure"
    );
  });
});

describe("non-invoke core members keep local semantics (A-13)", () => {
  it("re-exports convertFileSrc as the primitive's own function", () => {
    expect(wrapper.convertFileSrc).toBe(primitive.convertFileSrc);
  });

  it("re-exports Channel as the primitive's own class", () => {
    expect(wrapper.Channel).toBe(primitive.Channel);
  });

  it("re-exports transformCallback and isTauri unchanged", () => {
    expect(wrapper.transformCallback).toBe(primitive.transformCallback);
    expect(wrapper.isTauri).toBe(primitive.isTauri);
  });

  it("replaces ONLY invoke", () => {
    expect(wrapper.invoke).not.toBe(primitive.invoke);
  });

  it("drops no core export", () => {
    const missing = Object.keys(primitive).filter((key) => !(key in wrapper));
    expect(missing).toEqual([]);
  });
});

describe("remote routing", () => {
  beforeEach(() => {
    setTransportEnvironmentId(REMOTE_ENV);
  });

  it("dispatches an ordinary command through remote_invoke", async () => {
    primitiveInvoke.mockResolvedValue({ outcome: "ok", result: [{ id: "t1" }] });

    const result = await wrapper.invoke("list_tasks", { projectId: "p1" });

    expect(result).toEqual([{ id: "t1" }]);
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = primitiveInvoke.mock.calls[0] ?? [];
    expect(cmd).toBe("remote_invoke");
    const input = (args as { input: Record<string, unknown> }).input;
    expect(input.id).toBe(REMOTE_ENV);
    expect(input.cmd).toBe("list_tasks");
    expect(input.args).toEqual({ projectId: "p1" });
    expect(typeof input.requestId).toBe("string");
  });

  it("keeps client-owned commands on local IPC instead of proxying them", async () => {
    await wrapper.invoke("set_active_environment", { input: { id: "local" } });

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke).toHaveBeenCalledWith(
      "set_active_environment",
      { input: { id: "local" } },
      undefined
    );
  });

  it("never proxies remote_invoke itself, which would recurse", async () => {
    await wrapper.invoke("remote_invoke", { input: { id: REMOTE_ENV } });

    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
  });

  it("refuses host-path shell launches with a typed REMOTE_COMMAND_UNAVAILABLE", async () => {
    const error = await wrapper
      .invoke("open_agent_conversation_workspace", { conversationId: "c1" })
      .catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(RemoteTransportError);
    expect((error as RemoteTransportError).code).toBe("REMOTE_COMMAND_UNAVAILABLE");
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });

  it("routes locally again as soon as the environment switches back", async () => {
    resetTransportEnvironmentId();
    await wrapper.invoke("list_tasks", { projectId: "p1" });
    expect(primitiveInvoke).toHaveBeenCalledWith(
      "list_tasks",
      { projectId: "p1" },
      undefined
    );
  });
});

describe("typedInvoke rides the same seam (A-3)", () => {
  it("routes remotely without any call-site edit", async () => {
    setTransportEnvironmentId(REMOTE_ENV);
    primitiveInvoke.mockResolvedValue({ outcome: "ok", result: { status: "ok" } });
    // `lib/tauri.ts` imports `invoke` from "@tauri-apps/api/core"; under the alias
    // that specifier IS this module, so importing the wrapper's `invoke` and calling
    // it the way typedInvoke does exercises the identical path.
    const { z } = await import("zod");
    const schema = z.object({ status: z.string() });

    const raw = await wrapper.invoke("health_check", {});

    expect(schema.parse(raw)).toEqual({ status: "ok" });
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");
  });
});
