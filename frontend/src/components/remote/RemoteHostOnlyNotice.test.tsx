/**
 * The host-only banner names the machine that owns the setting.
 *
 * The regression it replaces: panes whose commands the facade refuses sat on their LOADING
 * state forever under a remote environment, which reads as a broken app rather than as a
 * capability boundary. The banner must therefore always answer "which machine?" — including
 * in the window where the registry entry is not yet resolvable, which is exactly when a
 * silent or blank banner would be most confusing.
 */

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { RemoteHostOnlyNotice } from "./RemoteHostOnlyNotice";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const BASE_URL = "http://100.95.136.117:3849";

function summary(name: string) {
  return {
    id: REMOTE_ID,
    environmentId: "host-1",
    name,
    baseUrl: BASE_URL,
    candidateUrls: [],
    scopes: ["ui:read"],
    protocolVersion: 1,
    status: "active" as const,
    createdAt: "2026-07-30T00:00:00Z",
    lastConnectedAt: null,
  };
}

function activateRemote(name: string): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name, kind: "remote", remote: summary(name) },
    ],
  });
}

beforeEach(() => {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
  });
});

describe("RemoteHostOnlyNotice", () => {
  it("names the subject and the host in the title, with the address beneath", () => {
    activateRemote("100.95.136.117:3849");
    render(<RemoteHostOnlyNotice subject="Provider setup" />);

    expect(
      screen.getByText("Provider setup runs on 100.95.136.117:3849"),
    ).toBeInTheDocument();
    expect(screen.getByText(BASE_URL)).toBeInTheDocument();
  });

  it("keeps the address visible when the host was renamed, so the machine stays identifiable", () => {
    activateRemote("Studio Mac");
    render(<RemoteHostOnlyNotice subject="Provider setup" />);

    expect(screen.getByText("Provider setup runs on Studio Mac")).toBeInTheDocument();
    expect(screen.getByText(BASE_URL)).toBeInTheDocument();
  });

  it("carries the warning tone rather than an error tone", () => {
    // This is not a failure — the setting simply lives elsewhere.
    activateRemote("Studio Mac");
    render(<RemoteHostOnlyNotice subject="Provider setup" testId="notice" />);

    expect(screen.getByTestId("notice")).toHaveAttribute("data-tone", "warning");
  });

  it("still names a host when the registry entry is not resolvable", () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-not-in-registry",
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    });
    render(<RemoteHostOnlyNotice subject="Provider setup" />);

    expect(
      screen.getByText("Provider setup runs on the remote host"),
    ).toBeInTheDocument();
  });
});
