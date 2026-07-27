import { describe, expect, it } from "vitest";
import {
  captureTranscriptSnapshot,
  expectPrefixExact,
  expectSameTranscript,
  textOnlyExposure,
  type TranscriptSnapshotRow,
} from "./transcriptSnapshot";

const transcript: TranscriptSnapshotRow[] = [
  { kind: "user", key: "user:Prompt", text: "Prompt" },
  { kind: "text", key: "text:Answer", text: "Answer" },
  { kind: "tool", key: "tool:grep", text: "grep" },
];

describe("transcriptSnapshot", () => {
  it("captures ordered text and stable tool identity", () => {
    const container = document.createElement("div");
    container.innerHTML = `
      <article data-chat-message-item="true" class="justify-end">
        <div data-testid="text-bubble-user">Prompt</div>
      </article>
      <article data-chat-message-item="true" class="justify-start">
        <div data-testid="tool-call-indicator"><button data-testid="tool-call-toggle" aria-label="Tool call: Grep. Click to expand.">Grep</button></div>
        <div data-testid="text-bubble-assistant">Answer</div>
      </article>
    `;

    expect(captureTranscriptSnapshot(container)).toEqual([
      transcript[0],
      transcript[2],
      transcript[1],
    ]);
  });

  it("rejects reordered rows", () => {
    expect(() => expectSameTranscript(
      [transcript[1]!, transcript[0]!, transcript[2]!],
      transcript,
    )).toThrow("Transcript differs at row 0");
  });

  it("rejects duplicate stable keys", () => {
    expect(() => expectSameTranscript(
      [...transcript, { kind: "tool", key: "tool:grep", text: "duplicate" }],
      transcript,
    )).toThrow("actual transcript has duplicate transcript key: tool:grep");
  });

  it("rejects a prefix that skips a middle row", () => {
    expect(() => expectPrefixExact(
      [transcript[0]!, transcript[2]!],
      transcript,
    )).toThrow("Transcript differs at row 1");
  });

  it("exposes text bubbles only, excluding tool-card arguments", () => {
    const container = document.createElement("div");
    container.innerHTML = `
      <article data-chat-message-item="true" class="justify-start">
        <div data-testid="text-bubble-assistant">Safe transcript text</div>
        <div data-testid="tool-call-indicator">{"path":"private.txt"}</div>
      </article>
    `;

    expect(textOnlyExposure(container)).toBe("Safe transcript text");
  });
});
