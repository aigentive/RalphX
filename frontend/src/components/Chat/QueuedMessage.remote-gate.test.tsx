/**
 * Phase 1 — the queued-message edit/delete controls are honest on a paired device.
 *
 * `delete_queued_agent_message` is not a facade op, so both affordances resolve
 * `unavailable` from ABSENCE. Rendering them anyway is what let a "deleted" turn still be
 * delivered and an "edited" one be delivered twice: the control looked live, the click ran,
 * the host never dropped the original.
 *
 * The negative assertions are on the CONTROLS, not on a disabled attribute — a disabled
 * button still answers a stale callback reference or a keyboard activation, and the review's
 * whole complaint is about affordances that produce nothing. Send now stays: its path
 * (`send_remote_chat_message`) IS registered.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueuedMessage } from "./QueuedMessage";
import {
  AGENT_GATED_AFFORDANCES,
  REMOTE_FACADE_OPS,
  REMOTE_UNAVAILABLE_HINT,
} from "@/lib/remote/agent-gate";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import type { QueuedMessage as QueuedMessageType } from "@/stores/chatStore";

const REMOTE_ID = "env-remote";

function useLocalEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
}

/** Fully granted on a healthy connection: any remaining gate is ABSENCE, not scope. */
function useRemoteEnvironment(scopes = ["ui:read", "ui:operate", "ui:agent"]): void {
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
    effectiveScopes: { [REMOTE_ID]: scopes },
  });
}

const message: QueuedMessageType = {
  id: "queued-1",
  content: "Queued turn",
  createdAt: new Date().toISOString(),
  isEditing: false,
  attachmentIds: [],
};

function renderQueued(onSendNow?: () => void) {
  return render(
    <QueuedMessage
      message={message}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      {...(onSendNow ? { onSendNow } : {})}
    />,
  );
}

beforeEach(() => {
  useLocalEnvironment();
});

describe("queued-message affordances on a paired device", () => {
  it("renders neither edit nor delete, and says why", () => {
    useRemoteEnvironment();
    renderQueued();

    expect(screen.queryByTestId("queued-message-edit")).toBeNull();
    expect(screen.queryByTestId("queued-message-delete")).toBeNull();
    expect(
      screen.getByTestId("queued-message-unavailable-hint"),
    ).toHaveTextContent(REMOTE_UNAVAILABLE_HINT);
  });

  it("uses the shared hint constant, not a per-surface phrasing", () => {
    useRemoteEnvironment();
    renderQueued();

    // Copy discipline: the two existing constants only.
    expect(screen.getByTestId("queued-message-unavailable-hint").textContent).toBe(
      REMOTE_UNAVAILABLE_HINT,
    );
  });

  it("stays hidden with a fully granted scope set — this is absence, not scope", () => {
    useRemoteEnvironment(["ui:read", "ui:operate", "ui:agent"]);
    renderQueued();

    expect(screen.queryByTestId("queued-message-delete")).toBeNull();
  });

  it("keeps Send now, whose path the host does serve", () => {
    useRemoteEnvironment();
    renderQueued(vi.fn());

    expect(screen.getByTestId("queued-message-send-now")).toBeInTheDocument();
  });

  it("still shows the queued turn itself — hiding it would hide real state", () => {
    useRemoteEnvironment();
    renderQueued();

    expect(screen.getByTestId("queued-message-content")).toHaveTextContent(
      "Queued turn",
    );
  });

  it("leaves the local environment fully operable and unhinted", () => {
    useLocalEnvironment();
    renderQueued(vi.fn());

    expect(screen.getByTestId("queued-message-edit")).toBeInTheDocument();
    expect(screen.getByTestId("queued-message-delete")).toBeInTheDocument();
    expect(screen.queryByTestId("queued-message-unavailable-hint")).toBeNull();
  });
});

describe("the gate rows are derived from absence", () => {
  it("names the op the callsite invokes, and the host does not expose it", () => {
    expect(AGENT_GATED_AFFORDANCES.queuedMessageDelete).toBe(
      "delete_queued_agent_message",
    );
    // Edit shares the row because delete is the step that decides it: the edit path is
    // delete-then-send, and a swallowed delete plus an unconditional send is the double turn.
    expect(AGENT_GATED_AFFORDANCES.queuedMessageEdit).toBe(
      "delete_queued_agent_message",
    );
    expect(REMOTE_FACADE_OPS["delete_queued_agent_message"]).toBeUndefined();
  });
});
