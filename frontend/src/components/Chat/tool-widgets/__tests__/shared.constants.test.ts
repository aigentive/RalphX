import { describe, it, expect } from "vitest";
import {
  parseMcpToolResult,
  parseMcpToolResultRaw,
  parseToolResultAsLines,
} from "../shared.constants";

describe("parseMcpToolResult", () => {
  it("unwraps MCP content array to parsed object", () => {
    const input = [{ type: "text", text: '{"title":"Hello"}' }];
    expect(parseMcpToolResult(input)).toEqual({ title: "Hello" });
  });

  it("passes through a plain object unchanged", () => {
    const input = { title: "Hello" };
    expect(parseMcpToolResult(input)).toEqual({ title: "Hello" });
  });

  it("returns empty object for empty array", () => {
    expect(parseMcpToolResult([])).toEqual({});
  });

  it("returns empty object for malformed JSON in text", () => {
    const input = [{ type: "text", text: "not json" }];
    expect(parseMcpToolResult(input)).toEqual({});
  });

  it("returns empty object for null", () => {
    expect(parseMcpToolResult(null)).toEqual({});
  });

  it("returns empty object for undefined", () => {
    expect(parseMcpToolResult(undefined)).toEqual({});
  });

  it("returns empty object for non-text content type", () => {
    const input = [{ type: "image", data: "base64data" }];
    expect(parseMcpToolResult(input)).toEqual({});
  });

  it("uses only the first content block when multiple exist", () => {
    const input = [
      { type: "text", text: '{"a":1}' },
      { type: "text", text: '{"b":2}' },
    ];
    expect(parseMcpToolResult(input)).toEqual({ a: 1 });
  });

  it("returns empty object for array of strings (non-MCP array)", () => {
    const input = ["hello", "world"];
    expect(parseMcpToolResult(input)).toEqual({});
  });

  it("returns empty object for array of numbers", () => {
    const input = [1, 2, 3];
    expect(parseMcpToolResult(input)).toEqual({});
  });

  it("parses plain JSON string to object", () => {
    expect(parseMcpToolResult('{"title":"Hello"}')).toEqual({ title: "Hello" });
  });

  it("returns empty object for non-JSON plain string", () => {
    expect(parseMcpToolResult("not json")).toEqual({});
  });
});

describe("parseMcpToolResultRaw", () => {
  it("parses plain JSON string to object", () => {
    expect(parseMcpToolResultRaw('{"title":"Hello"}')).toEqual({ title: "Hello" });
  });

  it("returns null for non-JSON plain string", () => {
    expect(parseMcpToolResultRaw("not json")).toBeNull();
  });

  it("passes through plain object unchanged", () => {
    const obj = { a: 1 };
    expect(parseMcpToolResultRaw(obj)).toBe(obj);
  });

  it("unwraps MCP content array", () => {
    const input = [{ type: "text", text: '{"items":[1,2,3]}' }];
    expect(parseMcpToolResultRaw(input)).toEqual({ items: [1, 2, 3] });
  });

  it("returns null for null", () => {
    expect(parseMcpToolResultRaw(null)).toBeNull();
  });
});

describe("parseToolResultAsLines", () => {
  it("reads an object text field as trimmed lines", () => {
    expect(parseToolResultAsLines({ text: " one \n\n two " })).toEqual(["one", "two"]);
  });

  it("reads an object content string as trimmed lines", () => {
    expect(parseToolResultAsLines({ content: " alpha \n beta " })).toEqual([
      "alpha",
      "beta",
    ]);
  });

  it("unwraps object content from MCP text blocks", () => {
    const content = [{ type: "text", text: " first \n second " }];
    expect(parseToolResultAsLines({ content })).toEqual(["first", "second"]);
  });
});
