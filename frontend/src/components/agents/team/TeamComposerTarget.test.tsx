import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { TeamComposerTarget } from "./TeamComposerTarget";

describe("TeamComposerTarget", () => {
  it("keeps Coordinator as the default and emits member or broadcast targets", async () => {
    const onValueChange = vi.fn();
    const user = userEvent.setup();
    render(
      <TeamComposerTarget
        members={[
          {
            id: "member-1",
            teamId: "team-1",
            name: "Scout",
            normalizedName: "scout",
            canonicalAgentName: "ralphx-general-explorer",
            roleSummary: "Investigates focused questions.",
            status: "idle",
            generation: 1,
          },
        ]}
        value={null}
        onValueChange={onValueChange}
      />,
    );

    const selector = screen.getByLabelText("Team message recipient");
    expect(selector).toHaveValue("coordinator");

    await user.selectOptions(selector, "member:Scout");
    expect(onValueChange).toHaveBeenLastCalledWith({
      kind: "member",
      memberName: "Scout",
    });

    await user.selectOptions(selector, "broadcast");
    expect(onValueChange).toHaveBeenLastCalledWith({ kind: "broadcast" });
  });
});
