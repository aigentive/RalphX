import { describe, it, expect } from "vitest";
import {
  AskUserQuestionOptionSchema,
  AskUserQuestionPayloadSchema,
  AskUserQuestionResponseSchema,
  AskUserQuestionPayloadListSchema,
  hasSelection,
  hasCustomResponse,
  isValidResponse,
  createSingleSelectResponse,
  createMultiSelectResponse,
  createCustomResponse,
  createSkippedResponse,
} from "./ask-user-question";

describe("AskUserQuestionOptionSchema", () => {
  const validOption = {
    label: "JWT tokens",
    description: "Use JSON Web Tokens for authentication",
  };

  it("should parse a valid option", () => {
    expect(() => AskUserQuestionOptionSchema.parse(validOption)).not.toThrow();
  });

  it("should parse an option with minimal fields", () => {
    const minOption = {
      label: "Option A",
      description: "",
    };
    expect(() => AskUserQuestionOptionSchema.parse(minOption)).not.toThrow();
  });

  it("should reject option with empty label", () => {
    expect(() =>
      AskUserQuestionOptionSchema.parse({ ...validOption, label: "" })
    ).toThrow();
  });

  it("should reject option missing label", () => {
    expect(() =>
      AskUserQuestionOptionSchema.parse({ description: "Test" })
    ).toThrow();
  });

  it("should allow option missing description", () => {
    expect(() =>
      AskUserQuestionOptionSchema.parse({ label: "Test" })
    ).not.toThrow();
  });

  it("should accept null description (backend serializes Option::None as null)", () => {
    const parsed = AskUserQuestionOptionSchema.parse({
      label: "Option A",
      description: null,
    });
    expect(parsed.description).toBeNull();
  });

  it("should accept null value (backend serializes Option::None as null)", () => {
    const parsed = AskUserQuestionOptionSchema.parse({
      label: "Option B",
      value: null,
    });
    expect(parsed.value).toBeNull();
  });

  it("should accept options with both value and description as null", () => {
    const parsed = AskUserQuestionOptionSchema.parse({
      label: "Option C",
      value: null,
      description: null,
    });
    expect(parsed.value).toBeNull();
    expect(parsed.description).toBeNull();
  });
});

describe("AskUserQuestionPayloadSchema", () => {
  const validPayload = {
    requestId: "req-abc",
    taskId: "task-123",
    question: "Which authentication method should we use?",
    header: "Auth method",
    options: [
      { label: "JWT tokens", description: "Use JSON Web Tokens" },
      { label: "Session cookies", description: "Use server-side sessions" },
    ],
    multiSelect: false,
  };

  it("should parse a valid payload", () => {
    expect(() => AskUserQuestionPayloadSchema.parse(validPayload)).not.toThrow();
  });

  it("should parse a payload without taskId (MCP flow)", () => {
    const { taskId: _, ...mcpPayload } = validPayload;
    expect(() => AskUserQuestionPayloadSchema.parse(mcpPayload)).not.toThrow();
  });

  it("should parse a payload with sessionId", () => {
    const withSession = { ...validPayload, sessionId: "session-xyz" };
    expect(() => AskUserQuestionPayloadSchema.parse(withSession)).not.toThrow();
  });

  it("should parse skip and batch progress fields", () => {
    const parsed = AskUserQuestionPayloadSchema.parse({
      ...validPayload,
      allowSkip: false,
      batchIndex: 2,
      batchTotal: 3,
    });
    expect(parsed.allowSkip).toBe(false);
    expect(parsed.batchIndex).toBe(2);
    expect(parsed.batchTotal).toBe(3);
  });

  it("should parse a multi-select payload", () => {
    const multiSelectPayload = {
      ...validPayload,
      multiSelect: true,
    };
    expect(() => AskUserQuestionPayloadSchema.parse(multiSelectPayload)).not.toThrow();
  });

  it("should parse payload with many options", () => {
    const manyOptions = {
      ...validPayload,
      options: [
        { label: "Option 1", description: "First option" },
        { label: "Option 2", description: "Second option" },
        { label: "Option 3", description: "Third option" },
        { label: "Option 4", description: "Fourth option" },
      ],
    };
    expect(() => AskUserQuestionPayloadSchema.parse(manyOptions)).not.toThrow();
  });

  it("should reject payload with empty requestId", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, requestId: "" })
    ).toThrow();
  });

  it("should reject payload with empty question", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, question: "" })
    ).toThrow();
  });

  it("should allow payload with empty header", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, header: "" })
    ).not.toThrow();
  });

  it("should allow payload with empty options array", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, options: [] })
    ).not.toThrow();
  });

  it("should allow payload with only one option", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({
        ...validPayload,
        options: [{ label: "Only one", description: "Single option" }],
      })
    ).not.toThrow();
  });

  it("should reject payload missing requestId", () => {
    const { requestId: _, ...noRequestId } = validPayload;
    // requestId is required - omitting it but also omitting taskId
    const { taskId: _t, ...bare } = noRequestId;
    expect(() => AskUserQuestionPayloadSchema.parse(bare)).toThrow();
  });

  it("should parse payload with null descriptions in options (MCP backend serializes None as null)", () => {
    const payload = {
      requestId: "req-mcp",
      sessionId: "session-123",
      question: "Pick an approach",
      header: null,
      options: [
        { label: "Option A", value: "a", description: null },
        { label: "Option B", value: "b", description: null },
        { label: "Option C", value: null, description: "Has description" },
      ],
      multiSelect: false,
    };
    const parsed = AskUserQuestionPayloadSchema.parse(payload);
    expect(parsed.options).toHaveLength(3);
    expect(parsed.options[0]!.description).toBeNull();
    expect(parsed.options[2]!.value).toBeNull();
  });

  it("should reject payload with non-boolean multiSelect", () => {
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, multiSelect: "true" })
    ).toThrow();
    expect(() =>
      AskUserQuestionPayloadSchema.parse({ ...validPayload, multiSelect: 1 })
    ).toThrow();
  });
});

describe("AskUserQuestionResponseSchema", () => {
  const validResponse = {
    taskId: "task-123",
    selectedOptions: ["JWT tokens"],
  };

  it("should parse a valid response with taskId", () => {
    expect(() => AskUserQuestionResponseSchema.parse(validResponse)).not.toThrow();
  });

  it("should parse a valid response with requestId (MCP flow)", () => {
    const mcpResponse = {
      requestId: "req-abc",
      selectedOptions: ["Option 1"],
    };
    expect(() => AskUserQuestionResponseSchema.parse(mcpResponse)).not.toThrow();
  });

  it("should parse a response with both requestId and taskId", () => {
    const bothResponse = {
      requestId: "req-abc",
      taskId: "task-123",
      selectedOptions: ["Option 1"],
    };
    expect(() => AskUserQuestionResponseSchema.parse(bothResponse)).not.toThrow();
  });

  it("should parse a response with multiple selections", () => {
    const multiResponse = {
      taskId: "task-123",
      selectedOptions: ["Option 1", "Option 2"],
    };
    expect(() => AskUserQuestionResponseSchema.parse(multiResponse)).not.toThrow();
  });

  it("should parse a response with custom response", () => {
    const customResponse = {
      taskId: "task-123",
      selectedOptions: [],
      customResponse: "I want to use a different approach",
    };
    expect(() => AskUserQuestionResponseSchema.parse(customResponse)).not.toThrow();
  });

  it("should parse a skipped response", () => {
    const parsed = AskUserQuestionResponseSchema.parse({
      requestId: "req-abc",
      selectedOptions: [],
      skipped: true,
    });
    expect(parsed.skipped).toBe(true);
  });

  it("should parse a response with empty selectedOptions", () => {
    const emptySelection = {
      selectedOptions: [],
    };
    expect(() => AskUserQuestionResponseSchema.parse(emptySelection)).not.toThrow();
  });

  it("should parse a response without customResponse", () => {
    const parsed = AskUserQuestionResponseSchema.parse(validResponse);
    expect(parsed.customResponse).toBeUndefined();
  });

  it("should reject response with empty taskId", () => {
    expect(() =>
      AskUserQuestionResponseSchema.parse({ ...validResponse, taskId: "" })
    ).toThrow();
  });

  it("should reject response with empty requestId", () => {
    expect(() =>
      AskUserQuestionResponseSchema.parse({ requestId: "", selectedOptions: ["test"] })
    ).toThrow();
  });

  it("should reject response missing selectedOptions", () => {
    expect(() =>
      AskUserQuestionResponseSchema.parse({ taskId: "task-123" })
    ).toThrow();
  });

  it("should reject response with non-array selectedOptions", () => {
    expect(() =>
      AskUserQuestionResponseSchema.parse({ ...validResponse, selectedOptions: "Option 1" })
    ).toThrow();
  });
});

describe("AskUserQuestionPayloadListSchema", () => {
  it("should parse empty array", () => {
    expect(AskUserQuestionPayloadListSchema.parse([])).toEqual([]);
  });

  it("should parse array of valid payloads", () => {
    const payloads = [
      {
        requestId: "req-1",
        taskId: "task-1",
        question: "Question 1?",
        header: "Q1",
        options: [
          { label: "A", description: "Option A" },
          { label: "B", description: "Option B" },
        ],
        multiSelect: false,
      },
      {
        requestId: "req-2",
        sessionId: "session-1",
        question: "Question 2?",
        header: "Q2",
        options: [
          { label: "C", description: "Option C" },
          { label: "D", description: "Option D" },
        ],
        multiSelect: true,
      },
    ];
    expect(() => AskUserQuestionPayloadListSchema.parse(payloads)).not.toThrow();
    expect(AskUserQuestionPayloadListSchema.parse(payloads)).toHaveLength(2);
  });

  it("should reject array with invalid payload", () => {
    const payloads = [
      {
        taskId: "task-1",
        // Missing requestId and other required fields
      },
    ];
    expect(() => AskUserQuestionPayloadListSchema.parse(payloads)).toThrow();
  });
});

describe("AskUserQuestion helper functions", () => {
  describe("hasSelection", () => {
    it("should return true when options are selected", () => {
      expect(hasSelection({ taskId: "task-1", selectedOptions: ["Option A"] })).toBe(true);
      expect(hasSelection({ taskId: "task-1", selectedOptions: ["A", "B"] })).toBe(true);
    });

    it("should return false when no options are selected", () => {
      expect(hasSelection({ taskId: "task-1", selectedOptions: [] })).toBe(false);
    });
  });

  describe("hasCustomResponse", () => {
    it("should return true when custom response is provided", () => {
      expect(
        hasCustomResponse({
          taskId: "task-1",
          selectedOptions: [],
          customResponse: "My custom answer",
        })
      ).toBe(true);
    });

    it("should return false when custom response is undefined", () => {
      expect(hasCustomResponse({ taskId: "task-1", selectedOptions: [] })).toBe(false);
    });

    it("should return false when custom response is empty string", () => {
      expect(
        hasCustomResponse({
          taskId: "task-1",
          selectedOptions: [],
          customResponse: "",
        })
      ).toBe(false);
    });
  });

  describe("isValidResponse", () => {
    it("should return true when options are selected", () => {
      expect(isValidResponse({ taskId: "task-1", selectedOptions: ["Option A"] })).toBe(true);
    });

    it("should return true when custom response is provided", () => {
      expect(
        isValidResponse({
          taskId: "task-1",
          selectedOptions: [],
          customResponse: "My answer",
        })
      ).toBe(true);
    });

    it("should return true when both are provided", () => {
      expect(
        isValidResponse({
          taskId: "task-1",
          selectedOptions: ["A"],
          customResponse: "Additional info",
        })
      ).toBe(true);
    });

    it("should return false when neither is provided", () => {
      expect(isValidResponse({ taskId: "task-1", selectedOptions: [] })).toBe(false);
      expect(
        isValidResponse({
          taskId: "task-1",
          selectedOptions: [],
          customResponse: "",
        })
      ).toBe(false);
    });

    it("should return true when the question is skipped", () => {
      expect(
        isValidResponse({
          taskId: "task-1",
          selectedOptions: [],
          skipped: true,
        })
      ).toBe(true);
    });
  });

  describe("createSingleSelectResponse", () => {
    it("should create response with taskId (legacy flow)", () => {
      const response = createSingleSelectResponse("Option A", { taskId: "task-123" });
      expect(response.taskId).toBe("task-123");
      expect(response.selectedOptions).toEqual(["Option A"]);
      expect(response.customResponse).toBeUndefined();
    });

    it("should create response with requestId (MCP flow)", () => {
      const response = createSingleSelectResponse("Option A", { requestId: "req-abc" });
      expect(response.requestId).toBe("req-abc");
      expect(response.selectedOptions).toEqual(["Option A"]);
    });
  });

  describe("createMultiSelectResponse", () => {
    it("should create response with multiple options", () => {
      const response = createMultiSelectResponse(["A", "B", "C"], { taskId: "task-456" });
      expect(response.taskId).toBe("task-456");
      expect(response.selectedOptions).toEqual(["A", "B", "C"]);
      expect(response.customResponse).toBeUndefined();
    });

    it("should create response with empty options array", () => {
      const response = createMultiSelectResponse([], { requestId: "req-xyz" });
      expect(response.selectedOptions).toEqual([]);
    });
  });

  describe("createCustomResponse", () => {
    it("should create response with custom text", () => {
      const response = createCustomResponse("My custom answer", { taskId: "task-abc" });
      expect(response.taskId).toBe("task-abc");
      expect(response.selectedOptions).toEqual([]);
      expect(response.customResponse).toBe("My custom answer");
    });
  });

  describe("createSkippedResponse", () => {
    it("should create a skipped response", () => {
      const response = createSkippedResponse({ requestId: "req-skip" });
      expect(response.requestId).toBe("req-skip");
      expect(response.selectedOptions).toEqual([]);
      expect(response.skipped).toBe(true);
    });
  });
});
