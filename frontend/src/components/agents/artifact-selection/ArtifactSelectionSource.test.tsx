import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useArtifactSelectionStore } from "@/stores/artifactSelectionStore";
import { ArtifactSelectionSource } from "./ArtifactSelectionSource";

describe("ArtifactSelectionSource", () => {
  beforeEach(() => {
    useArtifactSelectionStore.getState().clearAllSelections();
  });

  it("commits a whole-line range snapshot for the current conversation", async () => {
    render(
      <ArtifactSelectionSource
        conversationId="conversation-1"
        source={{
          sourceType: "artifact",
          sourceKind: "plan",
          sourceId: "artifact-version-2",
          sourceTitle: "Implementation Plan",
          artifactVersion: 2,
        }}
        content={"first\r\nsecond\r\nthird\n"}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select plan lines" }));
    fireEvent.click(await screen.findByRole("button", { name: "Line 2: second" }));
    fireEvent.click(await screen.findByRole("button", { name: "Line 3: third" }), {
      shiftKey: true,
    });

    expect(
      useArtifactSelectionStore.getState().selections["conversation-1"],
    ).toEqual({
      sourceType: "artifact",
      sourceKind: "plan",
      sourceId: "artifact-version-2",
      sourceTitle: "Implementation Plan",
      artifactVersion: 2,
      startLine: 2,
      endLine: 3,
      content: "second\nthird",
    });
    expect(screen.getByText("Selected L2–3")).toBeTruthy();
  });

  it("reflects a clear from the shared composer selection store", async () => {
    render(
      <ArtifactSelectionSource
        conversationId="conversation-1"
        source={{
          sourceType: "ticket",
          sourceKind: "linear",
          sourceId: "issue-1",
          sourceKey: "ENG-1",
          provider: "linear",
        }}
        content={"first line\nsecond line"}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select ticket lines" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Line 1: first line" }),
    );
    expect(screen.getByText("Selected L1")).toBeTruthy();

    act(() => {
      useArtifactSelectionStore.getState().clearSelection("conversation-1");
    });

    expect(screen.queryByText("Selected L1")).toBeNull();

    fireEvent.click(
      await screen.findByRole("button", { name: "Line 2: second line" }),
      { shiftKey: true },
    );
    expect(
      useArtifactSelectionStore.getState().selections["conversation-1"],
    ).toEqual(expect.objectContaining({ startLine: 2, endLine: 2 }));
  });

  it("removes terminal document newlines before exposing selectable lines", async () => {
    render(
      <ArtifactSelectionSource
        conversationId="conversation-1"
        source={{
          sourceType: "artifact",
          sourceKind: "plan",
          sourceId: "plan-1",
        }}
        content={"first\nsecond\n\n"}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select plan lines" }));

    expect(
      await screen.findByRole("button", { name: "Line 2: second" }),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Line 3: Blank line" })).toBeNull();
  });
});
