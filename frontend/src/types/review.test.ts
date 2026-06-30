import { describe, it, expect } from "vitest";
import {
  ReviewerTypeSchema,
  ReviewStatusSchema,
  ReviewActionTypeSchema,
  ReviewOutcomeSchema,
  ReviewSchema,
  ReviewActionSchema,
  ReviewNoteSchema,
  ReviewListSchema,
  ReviewSettingsSchema,
  DEFAULT_REVIEW_SETTINGS,
  REVIEWER_TYPE_VALUES,
  REVIEW_STATUS_VALUES,
  REVIEW_ACTION_TYPE_VALUES,
  REVIEW_OUTCOME_VALUES,
  isReviewPending,
  isReviewComplete,
  isReviewApproved,
  shouldRunAiReview,
  shouldAutoCreateFix,
  needsHumanReview,
  needsFixApproval,
  exceededMaxAttempts,
  isOutcomePositive,
} from "./review";

describe("ReviewerTypeSchema", () => {
  it("should have 3 reviewer types", () => {
    expect(REVIEWER_TYPE_VALUES.length).toBe(3);
  });

  it("should parse valid reviewer types", () => {
    expect(ReviewerTypeSchema.parse("ai")).toBe("ai");
    expect(ReviewerTypeSchema.parse("human")).toBe("human");
    expect(ReviewerTypeSchema.parse("system")).toBe("system");
  });

  it("should reject invalid reviewer types", () => {
    expect(() => ReviewerTypeSchema.parse("robot")).toThrow();
    expect(() => ReviewerTypeSchema.parse("AI")).toThrow();
    expect(() => ReviewerTypeSchema.parse("")).toThrow();
  });
});

describe("ReviewStatusSchema", () => {
  it("should have 4 review statuses", () => {
    expect(REVIEW_STATUS_VALUES.length).toBe(4);
  });

  it("should parse valid review statuses", () => {
    expect(ReviewStatusSchema.parse("pending")).toBe("pending");
    expect(ReviewStatusSchema.parse("approved")).toBe("approved");
    expect(ReviewStatusSchema.parse("changes_requested")).toBe("changes_requested");
    expect(ReviewStatusSchema.parse("rejected")).toBe("rejected");
  });

  it("should reject invalid review statuses", () => {
    expect(() => ReviewStatusSchema.parse("waiting")).toThrow();
    expect(() => ReviewStatusSchema.parse("Pending")).toThrow();
    expect(() => ReviewStatusSchema.parse("")).toThrow();
  });
});

describe("ReviewActionTypeSchema", () => {
  it("should have 3 action types", () => {
    expect(REVIEW_ACTION_TYPE_VALUES.length).toBe(3);
  });

  it("should parse valid action types", () => {
    expect(ReviewActionTypeSchema.parse("created_fix_task")).toBe("created_fix_task");
    expect(ReviewActionTypeSchema.parse("moved_to_backlog")).toBe("moved_to_backlog");
    expect(ReviewActionTypeSchema.parse("approved")).toBe("approved");
  });

  it("should reject invalid action types", () => {
    expect(() => ReviewActionTypeSchema.parse("rejected")).toThrow();
    expect(() => ReviewActionTypeSchema.parse("CreatedFixTask")).toThrow();
  });
});

describe("ReviewOutcomeSchema", () => {
  it("should have 4 outcomes", () => {
    expect(REVIEW_OUTCOME_VALUES.length).toBe(4);
  });

  it("should parse valid outcomes", () => {
    expect(ReviewOutcomeSchema.parse("approved")).toBe("approved");
    expect(ReviewOutcomeSchema.parse("approved_no_changes")).toBe("approved_no_changes");
    expect(ReviewOutcomeSchema.parse("changes_requested")).toBe("changes_requested");
    expect(ReviewOutcomeSchema.parse("rejected")).toBe("rejected");
  });

  it("should reject invalid outcomes", () => {
    expect(() => ReviewOutcomeSchema.parse("pending")).toThrow();
    expect(() => ReviewOutcomeSchema.parse("Approved")).toThrow();
  });
});

describe("isOutcomePositive", () => {
  it("should return true for approved", () => {
    expect(isOutcomePositive("approved")).toBe(true);
  });

  it("should return true for approved_no_changes", () => {
    expect(isOutcomePositive("approved_no_changes")).toBe(true);
  });

  it("should return false for negative outcomes", () => {
    expect(isOutcomePositive("changes_requested")).toBe(false);
    expect(isOutcomePositive("rejected")).toBe(false);
  });
});

describe("ReviewSchema", () => {
  const validReview = {
    id: "550e8400-e29b-41d4-a716-446655440000",
    projectId: "project-123",
    taskId: "task-456",
    reviewerType: "ai" as const,
    status: "pending" as const,
    notes: null,
    createdAt: "2026-01-24T12:00:00Z",
    completedAt: null,
  };

  it("should parse a valid review", () => {
    expect(() => ReviewSchema.parse(validReview)).not.toThrow();
  });

  it("should parse a review with all fields", () => {
    const reviewWithAllFields = {
      ...validReview,
      status: "approved" as const,
      notes: "Looks good!",
      completedAt: "2026-01-24T13:00:00Z",
    };
    expect(() => ReviewSchema.parse(reviewWithAllFields)).not.toThrow();
  });

  it("should parse all valid statuses", () => {
    for (const status of REVIEW_STATUS_VALUES) {
      expect(() =>
        ReviewSchema.parse({ ...validReview, status })
      ).not.toThrow();
    }
  });

  it("should parse all valid reviewer types", () => {
    for (const reviewerType of REVIEWER_TYPE_VALUES) {
      expect(() =>
        ReviewSchema.parse({ ...validReview, reviewerType })
      ).not.toThrow();
    }
  });

  it("should reject review with empty id", () => {
    expect(() => ReviewSchema.parse({ ...validReview, id: "" })).toThrow();
  });

  it("should reject review with empty projectId", () => {
    expect(() => ReviewSchema.parse({ ...validReview, projectId: "" })).toThrow();
  });

  it("should reject review with empty taskId", () => {
    expect(() => ReviewSchema.parse({ ...validReview, taskId: "" })).toThrow();
  });

  it("should reject review with invalid status", () => {
    expect(() =>
      ReviewSchema.parse({ ...validReview, status: "invalid" })
    ).toThrow();
  });

  it("should reject review with invalid reviewerType", () => {
    expect(() =>
      ReviewSchema.parse({ ...validReview, reviewerType: "invalid" })
    ).toThrow();
  });

  it("should reject review missing required fields", () => {
    expect(() => ReviewSchema.parse({})).toThrow();
    expect(() => ReviewSchema.parse({ id: "test" })).toThrow();
  });
});

describe("ReviewActionSchema", () => {
  const validAction = {
    id: "550e8400-e29b-41d4-a716-446655440001",
    reviewId: "550e8400-e29b-41d4-a716-446655440000",
    actionType: "approved" as const,
    targetTaskId: null,
    createdAt: "2026-01-24T12:00:00Z",
  };

  it("should parse a valid action", () => {
    expect(() => ReviewActionSchema.parse(validAction)).not.toThrow();
  });

  it("should parse an action with targetTaskId", () => {
    const actionWithTarget = {
      ...validAction,
      actionType: "created_fix_task" as const,
      targetTaskId: "fix-task-123",
    };
    expect(() => ReviewActionSchema.parse(actionWithTarget)).not.toThrow();
  });

  it("should parse all valid action types", () => {
    for (const actionType of REVIEW_ACTION_TYPE_VALUES) {
      expect(() =>
        ReviewActionSchema.parse({ ...validAction, actionType })
      ).not.toThrow();
    }
  });

  it("should reject action with empty id", () => {
    expect(() => ReviewActionSchema.parse({ ...validAction, id: "" })).toThrow();
  });

  it("should reject action with empty reviewId", () => {
    expect(() =>
      ReviewActionSchema.parse({ ...validAction, reviewId: "" })
    ).toThrow();
  });

  it("should reject action with invalid actionType", () => {
    expect(() =>
      ReviewActionSchema.parse({ ...validAction, actionType: "invalid" })
    ).toThrow();
  });
});

describe("ReviewNoteSchema", () => {
  const validNote = {
    id: "550e8400-e29b-41d4-a716-446655440002",
    taskId: "task-456",
    reviewer: "ai" as const,
    outcome: "approved" as const,
    notes: null,
    createdAt: "2026-01-24T12:00:00Z",
  };

  it("should parse a valid note", () => {
    expect(() => ReviewNoteSchema.parse(validNote)).not.toThrow();
  });

  it("should parse a note with notes text", () => {
    const noteWithText = {
      ...validNote,
      notes: "Missing unit tests",
    };
    expect(() => ReviewNoteSchema.parse(noteWithText)).not.toThrow();
  });

  it("should parse all valid outcomes", () => {
    for (const outcome of REVIEW_OUTCOME_VALUES) {
      expect(() =>
        ReviewNoteSchema.parse({ ...validNote, outcome })
      ).not.toThrow();
    }
  });

  it("should parse all valid reviewer types", () => {
    for (const reviewer of REVIEWER_TYPE_VALUES) {
      expect(() =>
        ReviewNoteSchema.parse({ ...validNote, reviewer })
      ).not.toThrow();
    }
  });

  it("should reject note with empty id", () => {
    expect(() => ReviewNoteSchema.parse({ ...validNote, id: "" })).toThrow();
  });

  it("should reject note with empty taskId", () => {
    expect(() => ReviewNoteSchema.parse({ ...validNote, taskId: "" })).toThrow();
  });

  it("should reject note with invalid outcome", () => {
    expect(() =>
      ReviewNoteSchema.parse({ ...validNote, outcome: "pending" })
    ).toThrow();
  });

  it("should reject note with invalid reviewer", () => {
    expect(() =>
      ReviewNoteSchema.parse({ ...validNote, reviewer: "robot" })
    ).toThrow();
  });
});

describe("ReviewListSchema", () => {
  it("should parse empty array", () => {
    expect(ReviewListSchema.parse([])).toEqual([]);
  });

  it("should parse array of valid reviews", () => {
    const reviews = [
      {
        id: "review-1",
        projectId: "project-1",
        taskId: "task-1",
        reviewerType: "ai" as const,
        status: "pending" as const,
        notes: null,
        createdAt: "2026-01-24T12:00:00Z",
        completedAt: null,
      },
      {
        id: "review-2",
        projectId: "project-1",
        taskId: "task-2",
        reviewerType: "human" as const,
        status: "approved" as const,
        notes: "LGTM",
        createdAt: "2026-01-24T12:00:00Z",
        completedAt: "2026-01-24T13:00:00Z",
      },
    ];
    expect(() => ReviewListSchema.parse(reviews)).not.toThrow();
    expect(ReviewListSchema.parse(reviews)).toHaveLength(2);
  });

  it("should reject array with invalid review", () => {
    const reviews = [
      {
        id: "review-1",
        // Missing required fields
      },
    ];
    expect(() => ReviewListSchema.parse(reviews)).toThrow();
  });
});

describe("Review helper functions", () => {
  describe("isReviewPending", () => {
    it("should return true for pending status", () => {
      expect(isReviewPending("pending")).toBe(true);
    });

    it("should return false for non-pending statuses", () => {
      expect(isReviewPending("approved")).toBe(false);
      expect(isReviewPending("changes_requested")).toBe(false);
      expect(isReviewPending("rejected")).toBe(false);
    });
  });

  describe("isReviewComplete", () => {
    it("should return false for pending status", () => {
      expect(isReviewComplete("pending")).toBe(false);
    });

    it("should return true for completed statuses", () => {
      expect(isReviewComplete("approved")).toBe(true);
      expect(isReviewComplete("changes_requested")).toBe(true);
      expect(isReviewComplete("rejected")).toBe(true);
    });
  });

  describe("isReviewApproved", () => {
    it("should return true for approved status", () => {
      expect(isReviewApproved("approved")).toBe(true);
    });

    it("should return false for non-approved statuses", () => {
      expect(isReviewApproved("pending")).toBe(false);
      expect(isReviewApproved("changes_requested")).toBe(false);
      expect(isReviewApproved("rejected")).toBe(false);
    });
  });
});

describe("ReviewSettingsSchema", () => {
  it("should have correct default values", () => {
    expect(DEFAULT_REVIEW_SETTINGS.aiReviewEnabled).toBe(true);
    expect(DEFAULT_REVIEW_SETTINGS.aiReviewAutoFix).toBe(true);
    expect(DEFAULT_REVIEW_SETTINGS.requireFixApproval).toBe(false);
    expect(DEFAULT_REVIEW_SETTINGS.requireHumanReview).toBe(false);
    expect(DEFAULT_REVIEW_SETTINGS.requireWorkspaceReview).toBe(true);
    expect(DEFAULT_REVIEW_SETTINGS.maxFixAttempts).toBe(3);
  });

  it("should parse valid settings", () => {
    const settings = {
      aiReviewEnabled: true,
      aiReviewAutoFix: true,
      requireFixApproval: false,
      requireHumanReview: false,
      requireWorkspaceReview: true,
      maxFixAttempts: 3,
    };
    expect(() => ReviewSettingsSchema.parse(settings)).not.toThrow();
  });

  it("should apply defaults for missing fields", () => {
    const result = ReviewSettingsSchema.parse({});
    expect(result.aiReviewEnabled).toBe(true);
    expect(result.aiReviewAutoFix).toBe(true);
    expect(result.requireFixApproval).toBe(false);
    expect(result.requireHumanReview).toBe(false);
    expect(result.requireWorkspaceReview).toBe(true);
    expect(result.maxFixAttempts).toBe(3);
  });

  it("should apply defaults for partial data", () => {
    const result = ReviewSettingsSchema.parse({
      aiReviewEnabled: false,
      maxFixAttempts: 5,
    });
    expect(result.aiReviewEnabled).toBe(false);
    expect(result.aiReviewAutoFix).toBe(true);
    expect(result.requireFixApproval).toBe(false);
    expect(result.requireHumanReview).toBe(false);
    expect(result.requireWorkspaceReview).toBe(true);
    expect(result.maxFixAttempts).toBe(5);
  });

  it("should reject invalid maxFixAttempts", () => {
    expect(() =>
      ReviewSettingsSchema.parse({ maxFixAttempts: -1 })
    ).toThrow();
    expect(() =>
      ReviewSettingsSchema.parse({ maxFixAttempts: 1.5 })
    ).toThrow();
  });

  it("should reject non-boolean for boolean fields", () => {
    expect(() =>
      ReviewSettingsSchema.parse({ aiReviewEnabled: "true" })
    ).toThrow();
    expect(() =>
      ReviewSettingsSchema.parse({ requireHumanReview: 1 })
    ).toThrow();
  });
});

describe("ReviewSettings helper functions", () => {
  describe("shouldRunAiReview", () => {
    it("should return true when AI review is enabled", () => {
      expect(shouldRunAiReview({ ...DEFAULT_REVIEW_SETTINGS, aiReviewEnabled: true })).toBe(true);
    });

    it("should return false when AI review is disabled", () => {
      expect(shouldRunAiReview({ ...DEFAULT_REVIEW_SETTINGS, aiReviewEnabled: false })).toBe(false);
    });
  });

  describe("shouldAutoCreateFix", () => {
    it("should return true when auto fix is enabled", () => {
      expect(shouldAutoCreateFix({ ...DEFAULT_REVIEW_SETTINGS, aiReviewAutoFix: true })).toBe(true);
    });

    it("should return false when auto fix is disabled", () => {
      expect(shouldAutoCreateFix({ ...DEFAULT_REVIEW_SETTINGS, aiReviewAutoFix: false })).toBe(false);
    });
  });

  describe("needsHumanReview", () => {
    it("should return true when human review is required", () => {
      expect(needsHumanReview({ ...DEFAULT_REVIEW_SETTINGS, requireHumanReview: true })).toBe(true);
    });

    it("should return false when human review is not required", () => {
      expect(needsHumanReview({ ...DEFAULT_REVIEW_SETTINGS, requireHumanReview: false })).toBe(false);
    });
  });

  describe("needsFixApproval", () => {
    it("should return true when fix approval is required", () => {
      expect(needsFixApproval({ ...DEFAULT_REVIEW_SETTINGS, requireFixApproval: true })).toBe(true);
    });

    it("should return false when fix approval is not required", () => {
      expect(needsFixApproval({ ...DEFAULT_REVIEW_SETTINGS, requireFixApproval: false })).toBe(false);
    });
  });

  describe("exceededMaxAttempts", () => {
    it("should return false when under max attempts", () => {
      const settings = { ...DEFAULT_REVIEW_SETTINGS, maxFixAttempts: 3 };
      expect(exceededMaxAttempts(settings, 0)).toBe(false);
      expect(exceededMaxAttempts(settings, 1)).toBe(false);
      expect(exceededMaxAttempts(settings, 2)).toBe(false);
    });

    it("should return true when at or over max attempts", () => {
      const settings = { ...DEFAULT_REVIEW_SETTINGS, maxFixAttempts: 3 };
      expect(exceededMaxAttempts(settings, 3)).toBe(true);
      expect(exceededMaxAttempts(settings, 5)).toBe(true);
    });

    it("should work with custom max attempts", () => {
      const settings = { ...DEFAULT_REVIEW_SETTINGS, maxFixAttempts: 1 };
      expect(exceededMaxAttempts(settings, 0)).toBe(false);
      expect(exceededMaxAttempts(settings, 1)).toBe(true);
    });
  });
});
