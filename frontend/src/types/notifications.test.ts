import { describe, expect, it } from "vitest";

import { AttentionItemListSchema, NotificationSchema } from "./notifications";

describe("AttentionItemListSchema", () => {
  it("keeps the notification center safe when the backend adds an unknown category", () => {
    const items = AttentionItemListSchema.parse([{
      id: "future:1", category: "future_category", title: "Future attention item", target: { kind: "none" },
    }]);
    expect(items[0]?.category).toBe("info");
  });
});

describe("NotificationSchema", () => {
  it("degrades an unknown durable-history category to the neutral presentation", () => {
    const notification = NotificationSchema.parse({
      id: "future:1", createdAt: "2026-07-10T10:00:00Z", category: "future_category",
      severity: "info", title: "Future notification", target: { kind: "none" },
    });
    expect(notification.category).toBe("info");
  });
});
