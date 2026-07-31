/**
 * The remote half of the workspace shell's two boot reads.
 *
 * Like `chat-remote-transcript.test.ts`, this exercises the REAL transport wrapper —
 * `@tauri-apps/api/core` is aliased to `lib/remote/invoke`, so a remote environment routes
 * through `networkInvoke` and out via the `remote_invoke` primitive. Mocking the API module
 * would prove nothing about the routing, which is the only thing in question.
 *
 * What is being pinned: `list_projects` is ledgered Elevated (its `project_response` runs
 * `inspect_repository_capability`) and `get_agent_provider_settings` is Denied, so both are
 * unregistered on the facade. A client that calls them remotely gets
 * REMOTE_COMMAND_UNAVAILABLE, reads it as an empty workspace, and renders first-run
 * onboarding over a populated host — which is exactly what shipped. The spawn-free twins in
 * `commands/remote_workspace_commands.rs` are what a paired device must call instead.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { projectsApi } from "@/api/projects";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";

/**
 * Snake_case on purpose: the host's `RemoteProjectView` carries no `rename_all`, matching
 * `ProjectResponse`, so the SAME Zod schema and transform parse both answers. The projection
 * differs from the local one only by the fields it drops — `repository_capability`, whose
 * computation is the spawn carrier — never by their names.
 */
const RAW_PROJECT = {
  id: "project-1",
  name: "RalphX",
  working_directory: "/Users/host/code/ralphx",
  git_mode: "worktree",
  base_branch: "main",
  use_feature_branches: true,
  merge_validation_mode: "block",
  github_pr_enabled: false,
  detected_analysis: null,
  custom_analysis: null,
  analyzed_at: null,
  created_at: "2026-07-30T12:00:00+00:00",
  updated_at: "2026-07-30T12:00:00+00:00",
} as const;

function useRemoteEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
    ],
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
    effectiveScopes: { [REMOTE_ID]: ["ui:read", "ui:operate"] },
  });
}

function useLocalEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
}

function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

function remoteOk(result: unknown): void {
  primitiveInvoke.mockResolvedValue({ outcome: "ok", result });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("workspace shell read routing", () => {
  it("lists projects via list_remote_projects, never the unregistered local one", async () => {
    useRemoteEnvironment();
    remoteOk([RAW_PROJECT]);

    const projects = await projectsApi.list();

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");
    expect(wireInput().cmd).toBe("list_remote_projects");
    expect(projects).toHaveLength(1);
    expect(projects[0]?.name).toBe("RalphX");
    expect(projects[0]?.workingDirectory).toBe("/Users/host/code/ralphx");
  });

  it("keeps calling the local list_projects on the local environment", async () => {
    primitiveInvoke.mockResolvedValue([RAW_PROJECT]);

    await projectsApi.list();

    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("list_projects");
  });

  it("reads provider readiness via the projection, never the Denied settings command", async () => {
    useRemoteEnvironment();
    remoteOk({ onboardingComplete: true, enabledProviderCount: 2 });

    const readiness = await projectsApi.remoteProviderReadiness();

    expect(wireInput().cmd).toBe("get_remote_provider_readiness");
    expect(readiness).toEqual({ onboardingComplete: true, enabledProviderCount: 2 });
  });

  it("surfaces a failed project read instead of returning an empty workspace", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockRejectedValue("REMOTE_INTERNAL_ERROR: host database is down");

    // The whole bug class: a rejected read that resolves to [] renders as "first run", so a
    // populated host would show onboarding. It must reject.
    await expect(projectsApi.list()).rejects.toBeDefined();
  });
});
