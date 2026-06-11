import { afterEach, describe, expect, it, vi } from "vitest";

import { handleAskUserQuestion } from "../question-handler.js";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function parsedToolText(result: { content: Array<{ type: "text"; text: string }> }): unknown {
  return JSON.parse(result.content[0]?.text ?? "{}");
}

describe("handleAskUserQuestion", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("keeps legacy single-question fields and adds a renderable answer record", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse({ request_id: "req-1" }))
      .mockResolvedValueOnce(
        jsonResponse({
          selected_options: ["yes"],
          text: null,
          skipped: false,
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    const result = await handleAskUserQuestion({
      session_id: "session-1",
      header: "Launch gate",
      question: "Proceed?",
      options: [{ label: "Yes", value: "yes" }],
    });

    expect(parsedToolText(result)).toEqual({
      selected_options: ["yes"],
      text: null,
      skipped: false,
      answers: [
        {
          id: "1",
          request_id: "req-1",
          header: "Launch gate",
          question: "Proceed?",
          options: [{ label: "Yes", value: "yes" }],
          selected_options: ["yes"],
          text: null,
          skipped: false,
        },
      ],
    });

    const requestBody = JSON.parse(
      fetchMock.mock.calls[0]?.[1]?.body as string
    );
    expect(requestBody).toMatchObject({
      session_id: "session-1",
      question: "Proceed?",
      allow_skip: true,
      multi_select: false,
    });
    expect(requestBody.batch_index).toBeUndefined();
    expect(requestBody.batch_total).toBeUndefined();
  });

  it("asks batched interview questions sequentially", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse({ request_id: "req-1" }))
      .mockResolvedValueOnce(
        jsonResponse({
          selected_options: ["backend"],
          text: null,
          skipped: false,
        })
      )
      .mockResolvedValueOnce(jsonResponse({ request_id: "req-2" }))
      .mockResolvedValueOnce(
        jsonResponse({
          selected_options: [],
          text: null,
          skipped: true,
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    const result = await handleAskUserQuestion({
      session_id: "session-1",
      questions: [
        {
          id: "scope",
          question: "Which area should we focus on?",
          options: [{ label: "Backend", value: "backend" }],
        },
        {
          id: "deadline",
          question: "Any deadline?",
          allow_skip: true,
        },
      ],
    });

    expect(parsedToolText(result)).toEqual({
      answers: [
        {
          id: "scope",
          request_id: "req-1",
          header: null,
          question: "Which area should we focus on?",
          options: [{ label: "Backend", value: "backend" }],
          selected_options: ["backend"],
          text: null,
          skipped: false,
        },
        {
          id: "deadline",
          request_id: "req-2",
          header: null,
          question: "Any deadline?",
          options: [],
          selected_options: [],
          text: null,
          skipped: true,
        },
      ],
    });

    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(String(fetchMock.mock.calls[1]?.[0])).toContain(
      "/api/question/await/req-1"
    );
    expect(String(fetchMock.mock.calls[3]?.[0])).toContain(
      "/api/question/await/req-2"
    );

    const firstRequestBody = JSON.parse(
      fetchMock.mock.calls[0]?.[1]?.body as string
    );
    const secondRequestBody = JSON.parse(
      fetchMock.mock.calls[2]?.[1]?.body as string
    );
    expect(firstRequestBody).toMatchObject({
      batch_index: 1,
      batch_total: 2,
      allow_skip: true,
    });
    expect(secondRequestBody).toMatchObject({
      batch_index: 2,
      batch_total: 2,
      allow_skip: true,
    });
  });

  it("rejects calls without a single question or batch", async () => {
    const fetchMock = vi.fn<typeof fetch>();
    vi.stubGlobal("fetch", fetchMock);

    const result = await handleAskUserQuestion({
      session_id: "session-1",
    });

    expect(parsedToolText(result)).toEqual({
      error: true,
      message:
        "ask_user_question requires either question or a non-empty questions[] array.",
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
