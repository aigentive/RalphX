import { beforeEach, describe, expect, it } from "vitest";
import {
  clearPostUpdatePreparing,
  markPostUpdatePreparing,
  POST_UPDATE_PREPARING_STORAGE_KEY,
  readFreshPostUpdatePreparingMarker,
} from "./postUpdatePreparing";

describe("postUpdatePreparing", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("stores a fresh post-update marker with version metadata", () => {
    markPostUpdatePreparing("0.12.3", 1_000);

    expect(readFreshPostUpdatePreparingMarker(1_500)).toEqual({
      startedAt: 1_000,
      version: "0.12.3",
    });
  });

  it("clears expired markers", () => {
    markPostUpdatePreparing("0.12.3", 1_000);

    expect(readFreshPostUpdatePreparingMarker(3 * 60 * 1_000)).toBeNull();
    expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();
  });

  it("ignores malformed markers", () => {
    localStorage.setItem(POST_UPDATE_PREPARING_STORAGE_KEY, "{bad json");

    expect(readFreshPostUpdatePreparingMarker()).toBeNull();
    expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();
  });

  it("removes the marker on clear", () => {
    markPostUpdatePreparing("0.12.3");

    clearPostUpdatePreparing();

    expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();
  });
});
