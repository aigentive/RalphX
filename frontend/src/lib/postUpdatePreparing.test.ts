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

  function blockLocalStorageAccess(): () => void {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      get() {
        throw new Error("localStorage unavailable");
      },
    });

    return () => {
      if (descriptor) {
        Object.defineProperty(globalThis, "localStorage", descriptor);
      }
    };
  }

  it("stores a fresh post-update marker with version metadata", () => {
    markPostUpdatePreparing("0.12.3", 1_000);

    expect(readFreshPostUpdatePreparingMarker(1_500)).toEqual({
      startedAt: 1_000,
      version: "0.12.3",
    });
  });

  it("stores a marker without version metadata when no version is provided", () => {
    markPostUpdatePreparing("", 1_000);

    expect(readFreshPostUpdatePreparingMarker(1_500)).toEqual({
      startedAt: 1_000,
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

  it("ignores markers with invalid shapes", () => {
    localStorage.setItem(
      POST_UPDATE_PREPARING_STORAGE_KEY,
      JSON.stringify("not an object"),
    );
    expect(readFreshPostUpdatePreparingMarker()).toBeNull();

    localStorage.setItem(
      POST_UPDATE_PREPARING_STORAGE_KEY,
      JSON.stringify({ startedAt: "1000", version: "0.12.3" }),
    );
    expect(readFreshPostUpdatePreparingMarker()).toBeNull();
  });

  it("treats blocked localStorage as an absent marker store", () => {
    const restoreLocalStorage = blockLocalStorageAccess();

    try {
      expect(() => markPostUpdatePreparing("0.12.3")).not.toThrow();
      expect(readFreshPostUpdatePreparingMarker()).toBeNull();
      expect(() => clearPostUpdatePreparing()).not.toThrow();
    } finally {
      restoreLocalStorage();
    }
  });

  it("removes the marker on clear", () => {
    markPostUpdatePreparing("0.12.3");

    clearPostUpdatePreparing();

    expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();
  });
});
