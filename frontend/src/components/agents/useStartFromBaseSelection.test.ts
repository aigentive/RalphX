import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LoadBranchBaseOptionsResult } from "@/components/shared/branchBaseOptions";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import {
  useStartFromBaseSelection,
  type UseStartFromBaseSelectionInput,
} from "./useStartFromBaseSelection";

const AGENT_BRANCH_REF = "ralphx/ralphx/agent-6c5acefd";
const AGENT_BRANCH_KEY = `local_branch:${AGENT_BRANCH_REF}`;

const { loadBranchBaseOptionsMock, loadPullRequestBaseOptionsMock } =
  vi.hoisted(() => ({
    loadBranchBaseOptionsMock: vi.fn(),
    loadPullRequestBaseOptionsMock: vi.fn(),
  }));

vi.mock("@/components/shared/branchBaseOptions", () => ({
  fallbackBranchBaseOptions: (baseBranch: string | null | undefined) => {
    const ref = baseBranch ?? "main";
    return {
      options: [projectDefaultOption(ref)],
      selectedKey: `project_default:${ref}`,
      degraded: { agentBranches: false, planBranches: false },
      knownBranchRefs: [ref],
    };
  },
  loadBranchBaseOptions: (...args: unknown[]) =>
    loadBranchBaseOptionsMock(...args),
  loadPullRequestBaseOptions: (...args: unknown[]) =>
    loadPullRequestBaseOptionsMock(...args),
  synthesizeLocalBranchOption: (ref: string, label?: string) => {
    const displayName = label ?? ref;
    return {
      key: `local_branch:${ref}`,
      label: displayName,
      detail: "Local branch",
      source: "local",
      selection: {
        kind: "local_branch",
        ref,
        displayName,
      },
    };
  },
}));

function projectDefaultOption(ref = "main") {
  return {
    key: `project_default:${ref}`,
    label: `Project default (${ref})`,
    detail: "Configured project base branch",
    source: "project" as const,
    selection: {
      kind: "project_default" as const,
      ref,
      displayName: `Project default (${ref})`,
    },
  };
}

function localBranchOption(ref = AGENT_BRANCH_REF) {
  return {
    key: `local_branch:${ref}`,
    label: ref,
    detail: "Local branch",
    source: "local" as const,
    selection: {
      kind: "local_branch" as const,
      ref,
      displayName: ref,
    },
  };
}

function branchLoadResult(
  options = [projectDefaultOption(), localBranchOption()],
  overrides: Partial<LoadBranchBaseOptionsResult> = {},
): LoadBranchBaseOptionsResult {
  return {
    options,
    selectedKey: "project_default:main",
    degraded: { agentBranches: false, planBranches: false },
    knownBranchRefs: ["main", AGENT_BRANCH_REF],
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve: (value: T) => resolve(value) };
}

function renderStartFromSelection(
  overrides: Partial<UseStartFromBaseSelectionInput> = {},
) {
  const input: UseStartFromBaseSelectionInput = {
    activeProjectId: "project-1",
    activeProjectBaseBranch: "main",
    activeProjectWorkingDirectory: "/tmp/project-1",
    clickupTicketToken: null,
    mode: "edit",
    onClearError: vi.fn(),
    ...overrides,
  };
  return renderHook(
    (props: UseStartFromBaseSelectionInput) => useStartFromBaseSelection(props),
    {
      initialProps: input,
    },
  );
}

describe("useStartFromBaseSelection", () => {
  beforeEach(() => {
    useAgentSessionStore.setState(useAgentSessionStore.getInitialState(), true);
    loadBranchBaseOptionsMock.mockReset();
    loadPullRequestBaseOptionsMock.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("in-flight refresh does not override an explicit pick", async () => {
    const options = [projectDefaultOption(), localBranchOption()];
    const pending = deferred<LoadBranchBaseOptionsResult>();
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "project_default:main",
      },
    });
    loadBranchBaseOptionsMock.mockReturnValue(pending.promise);
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.ensureStartFromOptionsLoaded();
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    await act(async () => {
      pending.resolve(branchLoadResult(options));
      await pending.promise;
    });

    expect(result.current.selectedStartFromKey).toBe(AGENT_BRANCH_KEY);
    expect(
      useAgentSessionStore.getState().lastBranchBaseSelectionByProjectId[
        "project-1"
      ],
    ).toBe(AGENT_BRANCH_KEY);
  });

  it("degraded refresh re-admits a still-existing selected ralphx branch", async () => {
    const options = [projectDefaultOption(), localBranchOption()];
    const pending = deferred<LoadBranchBaseOptionsResult>();
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    loadBranchBaseOptionsMock.mockReturnValue(pending.promise);
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
      result.current.ensureStartFromOptionsLoaded();
    });
    await act(async () => {
      pending.resolve(
        branchLoadResult([projectDefaultOption()], {
          degraded: { agentBranches: true, planBranches: false },
        }),
      );
      await pending.promise;
    });

    expect(result.current.startFromOptions).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ key: AGENT_BRANCH_KEY }),
      ]),
    );
  });

  it("degraded refresh does not re-admit a genuinely deleted branch", async () => {
    const options = [projectDefaultOption(), localBranchOption()];
    const pending = deferred<LoadBranchBaseOptionsResult>();
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    loadBranchBaseOptionsMock.mockReturnValue(pending.promise);
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
      result.current.ensureStartFromOptionsLoaded();
    });
    await act(async () => {
      pending.resolve(
        branchLoadResult([projectDefaultOption()], {
          degraded: { agentBranches: true, planBranches: false },
          knownBranchRefs: ["main"],
        }),
      );
      await pending.promise;
    });

    expect(result.current.startFromOptions).not.toEqual(
      expect.arrayContaining([
        expect.objectContaining({ key: AGENT_BRANCH_KEY }),
      ]),
    );
  });

  it("resolveBaseForSubmit retries before reporting unavailable", async () => {
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [projectDefaultOption()],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    loadBranchBaseOptionsMock.mockResolvedValue(
      branchLoadResult([projectDefaultOption(), localBranchOption()]),
    );
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    let resolution: Awaited<
      ReturnType<typeof result.current.resolveBaseForSubmit>
    >;
    await act(async () => {
      resolution = await result.current.resolveBaseForSubmit();
    });

    expect(loadBranchBaseOptionsMock).toHaveBeenCalledTimes(1);
    expect(resolution!).toEqual(
      expect.objectContaining({
        kind: "ok",
        base: expect.objectContaining({ ref: AGENT_BRANCH_REF }),
      }),
    );
  });

  it("resolveBaseForSubmit reports unavailable after a failed retry", async () => {
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [projectDefaultOption()],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    loadBranchBaseOptionsMock.mockResolvedValue(
      branchLoadResult([projectDefaultOption()], { knownBranchRefs: ["main"] }),
    );
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    let resolution: Awaited<
      ReturnType<typeof result.current.resolveBaseForSubmit>
    >;
    await act(async () => {
      resolution = await result.current.resolveBaseForSubmit();
    });

    expect(loadBranchBaseOptionsMock).toHaveBeenCalledTimes(1);
    expect(resolution!).toEqual({
      kind: "unavailable",
      base: null,
      unavailableRef: AGENT_BRANCH_REF,
    });
  });

  it("resolveBaseForSubmit does not retry without an explicit pick", async () => {
    const { result } = renderStartFromSelection();

    const resolution = await result.current.resolveBaseForSubmit();

    expect(loadBranchBaseOptionsMock).not.toHaveBeenCalled();
    expect(resolution).toEqual({
      kind: "ok",
      base: expect.objectContaining({
        kind: "project_default",
        ref: "main",
      }),
    });
  });

  it("isolation follows the surviving selection", async () => {
    const options = [projectDefaultOption(), localBranchOption()];
    const pending = deferred<LoadBranchBaseOptionsResult>();
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    loadBranchBaseOptionsMock.mockReturnValue(pending.promise);
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
      result.current.setIsolatedBranch(false);
      result.current.ensureStartFromOptionsLoaded();
    });
    await act(async () => {
      pending.resolve(
        branchLoadResult([projectDefaultOption()], {
          degraded: { agentBranches: true, planBranches: false },
        }),
      );
      await pending.promise;
    });

    expect(result.current.selectedStartFromKey).toBe(AGENT_BRANCH_KEY);
    expect(result.current.effectiveIsolatedBranch).toBe(false);
  });

  it("same-project rehydration preserves explicit intent", async () => {
    const options = [projectDefaultOption(), localBranchOption()];
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    const pending = deferred<LoadBranchBaseOptionsResult>();
    loadBranchBaseOptionsMock.mockReturnValue(pending.promise);
    const { result, rerender } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    // A baseBranch churn on the SAME project must not discard explicit intent.
    rerender({
      activeProjectId: "project-1",
      activeProjectBaseBranch: "trunk",
      activeProjectWorkingDirectory: "/tmp/project-1",
      clickupTicketToken: null,
      mode: "edit",
      onClearError: vi.fn(),
    });

    expect(result.current.selectedStartFromKey).toBe(AGENT_BRANCH_KEY);
    expect(result.current.effectiveIsolatedBranch).toBe(true);

    // Prove the intent flag itself survived: a refresh that prefers a
    // different remembered key still cannot override the pick.
    useAgentSessionStore.setState({
      lastBranchBaseSelectionByProjectId: {
        "project-1": "project_default:main",
      },
    });
    act(() => {
      result.current.ensureStartFromOptionsLoaded();
    });
    await act(async () => {
      pending.resolve(branchLoadResult(options));
    });

    expect(result.current.selectedStartFromKey).toBe(AGENT_BRANCH_KEY);
  });

  it("resolveBaseForSubmit rematches by ref when the option label changed", async () => {
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [projectDefaultOption()],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    const relabelled = {
      ...localBranchOption(),
      key: "renamed_key",
      label: "Fix workspace repair loops",
    };
    loadBranchBaseOptionsMock.mockResolvedValue(
      branchLoadResult([projectDefaultOption(), relabelled]),
    );
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    let resolution: Awaited<
      ReturnType<typeof result.current.resolveBaseForSubmit>
    >;
    await act(async () => {
      resolution = await result.current.resolveBaseForSubmit();
    });

    expect(resolution!).toEqual(
      expect.objectContaining({
        kind: "ok",
        base: expect.objectContaining({ ref: AGENT_BRANCH_REF }),
      }),
    );
  });

  it("resolveBaseForSubmit synthesizes from knownBranchRefs when the retry list omits the ref", async () => {
    useAgentSessionStore.setState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [projectDefaultOption()],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
    });
    // Retry list drops the branch, but git still reports it.
    loadBranchBaseOptionsMock.mockResolvedValue(
      branchLoadResult([projectDefaultOption()], {
        knownBranchRefs: ["main", AGENT_BRANCH_REF],
      }),
    );
    const { result } = renderStartFromSelection();

    act(() => {
      result.current.handleStartFromChange(AGENT_BRANCH_KEY);
    });
    let resolution: Awaited<
      ReturnType<typeof result.current.resolveBaseForSubmit>
    >;
    await act(async () => {
      resolution = await result.current.resolveBaseForSubmit();
    });

    expect(resolution!).toEqual(
      expect.objectContaining({
        kind: "ok",
        base: expect.objectContaining({
          kind: "local_branch",
          ref: AGENT_BRANCH_REF,
        }),
      }),
    );
  });
});
