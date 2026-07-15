import { describe, expect, it } from "vitest";

import {
  AttentionItemListSchema,
  NotificationPageSchema,
  NotificationSchema,
} from "./notifications";

describe("AttentionItemListSchema", () => {
  it("keeps the notification center safe when the backend adds an unknown category", () => {
    const items = AttentionItemListSchema.parse([{
      id: "future:1", category: "future_category", title: "Future attention item", target: { kind: "none" },
    }]);
    expect(items[0]?.category).toBe("info");
  });

  it("accepts serde-null attention optionals and normalizes them to undefined", () => {
    const [item] = AttentionItemListSchema.parse([{
      id: "attention:1",
      category: "task_failed",
      title: "Task failed",
      detail: null,
      projectId: null,
      createdAt: null,
      target: { kind: "none" },
    }]);

    expect(item).toMatchObject({
      detail: undefined,
      projectId: undefined,
      createdAt: undefined,
    });
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

  it("degrades an unknown durable-history severity to the neutral presentation", () => {
    const notification = NotificationSchema.parse({
      id: "future:1",
      createdAt: "2026-07-10T10:00:00Z",
      category: "info",
      severity: "future_severity",
      title: "Future notification",
      target: { kind: "none" },
    });

    expect(notification.severity).toBe("info");
  });

  it("accepts serde-null durable notification and page optionals and normalizes them to undefined", () => {
    const page = NotificationPageSchema.parse({
      notifications: [{
        id: "notification:1",
        createdAt: "2026-07-10T10:00:00Z",
        projectId: null,
        category: "task_failed",
        severity: "warning",
        title: "Task failed",
        body: null,
        target: { kind: "none" },
        dedupeKey: null,
        readAt: null,
      }],
      cursor: null,
      hasMore: false,
    });

    expect(page.notifications[0]).toMatchObject({
      projectId: undefined,
      body: undefined,
      dedupeKey: undefined,
      readAt: undefined,
    });
    expect(page.cursor).toBeUndefined();
  });
});
