import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

import {
  useActiveEnvironment,
  useActiveEnvironmentKind,
  useIsRemoteEnvironment,
} from "./useActiveEnvironment";

const REMOTE_SUMMARY = {
  id: "env-remote",
  name: "Studio Mac",
  status: "paired",
  scopes: ["ui:read", "ui:operate"],
} as unknown as NonNullable<
  ReturnType<
    typeof useEnvironmentStore.getState
  >["environments"][number]["remote"]
>;

function seedRemote(): void {
  useEnvironmentStore.setState({
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      {
        id: "env-remote",
        name: "Studio Mac",
        kind: "remote",
        remote: REMOTE_SUMMARY,
      },
    ],
  });
}

describe("useActiveEnvironment", () => {
  beforeEach(() => {
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
    });
  });

  it("returns the local entry by default", () => {
    const { result } = renderHook(() => useActiveEnvironment());
    expect(result.current.id).toBe(LOCAL_ENVIRONMENT_ID);
    expect(result.current.kind).toBe("local");
  });

  it("reports local kind and not-remote by default", () => {
    expect(renderHook(() => useActiveEnvironmentKind()).result.current).toBe(
      "local",
    );
    expect(renderHook(() => useIsRemoteEnvironment()).result.current).toBe(
      false,
    );
  });

  it("follows a switch to a remote environment", () => {
    seedRemote();
    const { result } = renderHook(() => useIsRemoteEnvironment());
    expect(result.current).toBe(false);

    act(() => {
      useEnvironmentStore.setState({ activeEnvironmentId: "env-remote" });
    });

    expect(result.current).toBe(true);
  });

  it("treats an unknown active id as remote (fail closed)", () => {
    // The registry can lag the Rust authority during hydration. Presuming "local"
    // for an id we cannot resolve would hand a remote session every host-only
    // affordance; presuming "remote" only costs a hidden button.
    act(() => {
      useEnvironmentStore.setState({
        activeEnvironmentId: "env-not-in-registry",
      });
    });
    expect(renderHook(() => useIsRemoteEnvironment()).result.current).toBe(
      true,
    );
    expect(renderHook(() => useActiveEnvironmentKind()).result.current).toBe(
      "remote",
    );
    expect(renderHook(() => useActiveEnvironment()).result.current).toBeNull();
  });

  it("keeps a stable reference across unrelated store churn", () => {
    seedRemote();
    act(() => {
      useEnvironmentStore.setState({ activeEnvironmentId: "env-remote" });
    });
    const { result, rerender } = renderHook(() => useActiveEnvironment());
    const first = result.current;

    act(() => {
      useEnvironmentStore
        .getState()
        .setConnectionState("env-remote", "backoff");
    });
    rerender();

    expect(result.current).toBe(first);
  });
});
