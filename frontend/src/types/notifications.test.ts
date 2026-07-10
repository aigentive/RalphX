import { describe, expect, it } from "vitest";

import { AttentionItemListSchema } from "./notifications";

describe("AttentionItemListSchema", () => {
  it("keeps the notification center safe when the backend adds an unknown category", () => {
    const items = AttentionItemListSchema.parse([{
      id: "future:1", category: "future_category", title: "Future attention item", target: { kind: "none" },
    }]);
    expect(items[0]?.category).toBe("info");
  });
});
