import { describe, expect, it } from "vitest";

import {
  ticketSelectClassName,
  ticketSelectOptionStyle,
  ticketSelectStyle,
} from "./ticket-select-styles";

describe("ticketSelectClassName", () => {
  it("maps the sm size to h-8 and not h-9", () => {
    const className = ticketSelectClassName("sm");
    expect(className).toContain("h-8");
    expect(className).not.toContain("h-9");
  });

  it("maps the md size to h-9 and not h-8", () => {
    const className = ticketSelectClassName("md");
    expect(className).toContain("h-9");
    expect(className).not.toContain("h-8");
  });

  it("defaults to the sm size", () => {
    expect(ticketSelectClassName()).toBe(ticketSelectClassName("sm"));
  });

  it("includes the caret class fragments for both sizes", () => {
    for (const className of [ticketSelectClassName("sm"), ticketSelectClassName("md")]) {
      expect(className).toContain("appearance-none");
      expect(className).toContain("bg-no-repeat");
      expect(className).toContain("pr-9");
    }
  });

  it("can omit native caret positioning for custom combobox triggers", () => {
    const className = ticketSelectClassName("sm", "pr-8", { nativeCaret: false });
    expect(className).toContain("appearance-none");
    expect(className).not.toContain("bg-no-repeat");
    expect(className).not.toContain("bg-[position:");
    expect(className.endsWith("pr-8")).toBe(true);
  });

  it("includes the app focus-ring fragment for both sizes", () => {
    for (const className of [ticketSelectClassName("sm"), ticketSelectClassName("md")]) {
      expect(className).toContain("focus-visible:[outline:2px_solid_var(--border-focus)]");
    }
  });

  it("uses a design-token caret color without raw color literals", () => {
    const className = ticketSelectClassName("sm");
    expect(className).toContain("var(--text-muted)");
    expect(className).not.toMatch(/hsla?\(/);
    expect(className).not.toContain("data:image/svg+xml");
  });

  it("appends caller-provided extra classes last", () => {
    const className = ticketSelectClassName("sm", "min-w-[150px]");
    expect(className.endsWith("min-w-[150px]")).toBe(true);
  });

  it("ignores an empty extra argument", () => {
    expect(ticketSelectClassName("sm", "")).toBe(ticketSelectClassName("sm"));
  });
});

describe("ticketSelectStyle", () => {
  it("uses paint/border longhands only (WKWebView Rule 6)", () => {
    const keys = Object.keys(ticketSelectStyle);
    expect(keys).toEqual(
      expect.arrayContaining([
        "backgroundColor",
        "borderColor",
        "borderStyle",
        "borderWidth",
        "color",
      ]),
    );
    // No `background`/`border` shorthand keys that WKWebView can drop.
    expect(keys).not.toContain("background");
    expect(keys).not.toContain("border");
  });

  it("resolves to the chosen theme tokens", () => {
    expect(ticketSelectStyle.backgroundColor).toBe("var(--bg-elevated)");
    expect(ticketSelectStyle.borderColor).toBe("var(--border-default)");
    expect(ticketSelectStyle.borderStyle).toBe("solid");
    expect(ticketSelectStyle.borderWidth).toBe("1px");
    expect(ticketSelectStyle.color).toBe("var(--text-primary)");
  });
});

describe("ticketSelectOptionStyle", () => {
  it("uses themed background and text tokens", () => {
    expect(ticketSelectOptionStyle.backgroundColor).toBe("var(--bg-elevated)");
    expect(ticketSelectOptionStyle.color).toBe("var(--text-primary)");
  });
});
