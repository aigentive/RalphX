/**
 * `backendFetch` — the environment-aware fetch seam (§6.2 point 3).
 *
 * Two properties carry the whole ~16-site migration: locally it is
 * indistinguishable from the `fetch(backendApiUrl(...))` it replaces, and remotely it
 * hands back a real `Response` so no call site can tell which transport served it.
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

import { backendApiPath, backendApiUrl, backendFetch } from "./backend";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { RemoteTransportError } from "@/lib/remote/transport-errors";

const REMOTE_ENV = "env-uuid-1";
const globalFetch = vi.fn<typeof fetch>();

function fetchInput(): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[0]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

beforeEach(() => {
  globalFetch.mockReset();
  globalFetch.mockResolvedValue(new Response("{}", { status: 200 }));
  global.fetch = globalFetch as unknown as typeof fetch;
  primitiveInvoke.mockReset();
  resetTransportEnvironmentId();
});

afterEach(() => {
  resetTransportEnvironmentId();
});

describe("path derivation", () => {
  it("keeps backendApiUrl and backendApiPath in agreement", () => {
    expect(backendApiUrl("agent_tasks/list")).toBe(
      `http://localhost:3847${backendApiPath("agent_tasks/list")}`
    );
  });

  it("applies the same guards to the remote path as to the local URL", () => {
    expect(() => backendApiPath("")).toThrow(/must not be empty/);
    expect(() => backendApiPath("https://evil.example/x")).toThrow(/Invalid/);
    expect(() => backendApiPath("//evil.example/x")).toThrow(/Invalid/);
    expect(() => backendApiPath("../internal/keys")).toThrow(/traversal/);
    expect(() => backendApiPath("diff?path=../../etc/passwd")).not.toThrow();
    expect(() => backendApiPath("../diff?path=fine")).toThrow(/traversal/);
  });

  // Call sites hand the query to the SAME argument (`endpoint?params`), and query values are
  // caller data: a workspace file named `notes..md` is not traversal. Rejecting it would break a
  // local fetch that worked before the transport migration — the parity the seam guarantees.
  it("leaves the query string intact instead of validating it as a path", () => {
    const params = new URLSearchParams({ path: "notes..md", ref_kind: "head" });

    expect(backendApiPath(`agent-workspaces/c1/file-diff-page?${params}`)).toBe(
      `/api/agent-workspaces/c1/file-diff-page?${params}`
    );
  });
});

describe("local environment", () => {
  it("calls fetch with the absolute URL and no second argument when none was given", async () => {
    await backendFetch("agent_tasks/list");

    expect(globalFetch).toHaveBeenCalledTimes(1);
    expect(globalFetch.mock.calls[0]).toEqual([
      backendApiUrl("agent_tasks/list"),
    ]);
  });

  it("forwards init by identity", async () => {
    const init: RequestInit = {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ a: 1 }),
    };

    await backendFetch("approve_plan_artifact", init);

    expect(globalFetch).toHaveBeenCalledWith(
      backendApiUrl("approve_plan_artifact"),
      init
    );
    expect(globalFetch.mock.calls[0]?.[1]).toBe(init);
  });

  it("never touches the remote proxy", async () => {
    await backendFetch("agent_tasks/list");
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });
});

describe("remote environment", () => {
  beforeEach(() => {
    setTransportEnvironmentId(REMOTE_ENV);
    primitiveInvoke.mockResolvedValue({ status: 200, body: '{"ok":true}' });
  });

  it("proxies through remote_fetch instead of the network", async () => {
    await backendFetch("agent_tasks/list");

    expect(globalFetch).not.toHaveBeenCalled();
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_fetch");
    const input = fetchInput();
    expect(input.id).toBe(REMOTE_ENV);
    expect(input.path).toBe("/api/agent_tasks/list");
    expect(input.method).toBe("GET");
    expect(input.headers).toEqual([]);
    expect(input.body).toBeUndefined();
  });

  it("carries method, headers, and body across the proxy", async () => {
    await backendFetch("approve_plan_artifact", {
      method: "post",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ session_id: "s1" }),
    });

    const input = fetchInput();
    expect(input.method).toBe("POST");
    expect(input.headers).toEqual([["Content-Type", "application/json"]]);
    expect(input.body).toBe(JSON.stringify({ session_id: "s1" }));
  });

  it("normalises a Headers instance into pairs", async () => {
    await backendFetch("x", {
      method: "POST",
      headers: new Headers({ "content-type": "application/json" }),
    });

    expect(fetchInput().headers).toEqual([
      ["content-type", "application/json"],
    ]);
  });

  it("rebuilds a real Response the call site can read", async () => {
    primitiveInvoke.mockResolvedValue({
      status: 200,
      body: JSON.stringify({ id: "task-1" }),
    });

    const response = await backendFetch("agent_tasks/list");

    expect(response.ok).toBe(true);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ id: "task-1" });
  });

  it("returns a non-2xx as a Response, not a rejection", async () => {
    primitiveInvoke.mockResolvedValue({
      status: 422,
      body: JSON.stringify({ error: "bad input" }),
    });

    const response = await backendFetch("agent_tasks/list");

    expect(response.ok).toBe(false);
    expect(response.status).toBe(422);
    expect(response.statusText).toBe("Unprocessable Content");
    await expect(response.json()).resolves.toEqual({ error: "bad input" });
  });

  it("builds a null-body Response for statuses that forbid one", async () => {
    primitiveInvoke.mockResolvedValue({ status: 204, body: "" });

    const response = await backendFetch("agent_tasks/list");

    expect(response.status).toBe(204);
    expect(response.body).toBeNull();
  });

  it("surfaces an auth refusal as a typed transport error", async () => {
    primitiveInvoke.mockRejectedValue(
      "REMOTE_UNAUTHORIZED: host refused this device's credential"
    );

    const error = await backendFetch("agent_tasks/list").catch(
      (reason: unknown) => reason
    );

    expect(error).toBeInstanceOf(RemoteTransportError);
    expect((error as RemoteTransportError).code).toBe("REMOTE_UNAUTHORIZED");
  });

  it("refuses a binary body rather than smuggling it through a JSON command", async () => {
    const error = await backendFetch("upload", {
      method: "POST",
      body: new Blob(["bytes"]),
    }).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(RemoteTransportError);
    expect((error as RemoteTransportError).code).toBe(
      "REMOTE_COMMAND_UNAVAILABLE"
    );
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });

  it("treats an unrecognised envelope as a version mismatch", async () => {
    primitiveInvoke.mockResolvedValue({ unexpected: true });

    const error = await backendFetch("agent_tasks/list").catch(
      (reason: unknown) => reason
    );

    expect((error as RemoteTransportError).code).toBe("REMOTE_VERSION_MISMATCH");
  });

  it("routes locally again once the environment switches back", async () => {
    resetTransportEnvironmentId();
    await backendFetch("agent_tasks/list");

    expect(globalFetch).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });
});
