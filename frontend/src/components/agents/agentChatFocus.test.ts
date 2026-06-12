import { describe, expect, it } from "vitest";

import {
  getAgentChatFocusSwitchOptions,
  type AgentsChatFocus,
} from "./agentChatFocus";

const verificationFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
  type: "verification",
  parentSessionId: "session-1",
  childSessionId: "verification-1",
};

describe("getAgentChatFocusSwitchOptions", () => {
  it("keeps the full ideation focus switcher in ideation mode", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "ideation",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "ideation",
      "verification",
    ]);
  });

  it("shows only verification as a child focus in plan mode when a plan and verification child exist", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "plan",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "verification",
    ]);
  });

  it("hides verification in plan mode until a plan exists", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "plan",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      hasPlanArtifact: false,
    });

    expect(options.map((option) => option.type)).toEqual(["workspace"]);
  });

  it("keeps non-planning modes workspace-only", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "edit",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual(["workspace"]);
  });
});
