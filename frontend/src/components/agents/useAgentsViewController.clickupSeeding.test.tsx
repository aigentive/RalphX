import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type {
  ComposerIntegrationReference,
  SendAgentMessageResult,
} from "@/api/chat";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { conversationFixture as conversation } from "./agentsTestFixtures";

type UserMessageSentHandler = (payload: {
  content: string;
  result: SendAgentMessageResult;
  composerIntegrationReferences?: ComposerIntegrationReference[];
}) => void | Promise<void>;

const { integratedChatPanelUserMessageSentMock } = getAgentsViewTestMocks();

function messageResult(conversationId: string): SendAgentMessageResult {
  return {
    conversationId,
    agentRunId: "run-1",
    isNewConversation: false,
    wasQueued: false,
    queuedAsPending: false,
    queuedMessageId: null,
  };
}

describe("useAgentsViewController ClickUp artifact seeding", () => {
  beforeEach(() => {
    setupAgentsViewTest();
    const selectedConversation = conversation({ id: "conversation-1" });
    mockAgentViewData(selectedConversation);
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: selectedConversation.id,
    });
  });

  async function capturedUserMessageSentHandler(): Promise<UserMessageSentHandler> {
    renderAgentsView();

    await waitFor(() => {
      expect(integratedChatPanelUserMessageSentMock).toHaveBeenCalled();
    });
    const onUserMessageSent = integratedChatPanelUserMessageSentMock.mock.lastCall?.[0] as
      | UserMessageSentHandler
      | undefined;
    expect(onUserMessageSent).toBeDefined();
    if (!onUserMessageSent) {
      throw new Error("IntegratedChatPanel did not receive onUserMessageSent");
    }
    return onUserMessageSent;
  }

  it("does not seed an artifact tab when a sent message has no integration references", async () => {
    const onUserMessageSent = await capturedUserMessageSentHandler();
    await onUserMessageSent({
      content: "Message without a linked artifact",
      result: messageResult("conversation-without-reference"),
    });
    expect(
      useAgentSessionStore.getState().artifactByConversationId[
        "conversation-without-reference"
      ],
    ).toBeUndefined();
  });

  it("seeds ClickUp when a sent message contains a ClickUp reference", async () => {
    const onUserMessageSent = await capturedUserMessageSentHandler();
    await onUserMessageSent({
      content: "Work on CU-42",
      result: messageResult("conversation-with-clickup"),
      composerIntegrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "task-42",
          key: "CU-42",
          title: "Restore rich artifact details",
        },
      ],
    });

    await waitFor(() => {
      expect(
        useAgentSessionStore.getState().artifactByConversationId[
          "conversation-with-clickup"
        ],
      ).toEqual(
        expect.objectContaining({
          isOpen: true,
          activeTab: "clickup",
        }),
      );
    });
  });
});
