import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useInputHistory } from "./useInputHistory";

const STORAGE_KEY = "ralphx:composer-input-history";

function makeTextarea(value: string, cursorPos?: number): HTMLTextAreaElement {
  const el = document.createElement("textarea");
  el.value = value;
  const pos = cursorPos ?? value.length;
  Object.defineProperty(el, "selectionStart", { value: pos, writable: true });
  Object.defineProperty(el, "selectionEnd", { value: pos, writable: true });
  return el;
}

function makeKeyEvent(
  key: string,
  textarea: HTMLTextAreaElement
): React.KeyboardEvent<HTMLTextAreaElement> {
  let defaultPrevented = false;
  return {
    key,
    currentTarget: textarea,
    target: textarea,
    preventDefault: () => {
      defaultPrevented = true;
    },
    get defaultPrevented() {
      return defaultPrevented;
    },
  } as unknown as React.KeyboardEvent<HTMLTextAreaElement>;
}

describe("useInputHistory", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("starts with no entries and does not navigate", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));
    const textarea = makeTextarea("");
    const event = makeKeyEvent("ArrowUp", textarea);
    let handled: boolean;
    act(() => {
      handled = result.current.handleHistoryKeyDown(event, "");
    });
    expect(handled!).toBe(false);
    expect(setValue).not.toHaveBeenCalled();
  });

  it("records sent messages and navigates up through them", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("first");
      result.current.addEntry("second");
      result.current.addEntry("third");
    });

    const textarea = makeTextarea("");

    act(() => {
      result.current.handleHistoryKeyDown(makeKeyEvent("ArrowUp", textarea), "");
    });
    expect(setValue).toHaveBeenLastCalledWith("third");

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        "third"
      );
    });
    expect(setValue).toHaveBeenLastCalledWith("second");

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        "second"
      );
    });
    expect(setValue).toHaveBeenLastCalledWith("first");
  });

  it("navigates back down to the draft value", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("alpha");
      result.current.addEntry("beta");
    });

    const textarea = makeTextarea("my draft");

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        "my draft"
      );
    });
    expect(setValue).toHaveBeenLastCalledWith("beta");

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowDown", makeTextarea("beta")),
        "beta"
      );
    });
    expect(setValue).toHaveBeenLastCalledWith("my draft");
  });

  it("does not navigate down past the draft", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("msg");
    });

    const textarea = makeTextarea("");

    let handled: boolean;
    act(() => {
      handled = result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowDown", textarea),
        ""
      );
    });
    expect(handled!).toBe(false);
    expect(setValue).not.toHaveBeenCalled();
  });

  it("does not navigate up past the oldest entry", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("only");
    });

    const textarea = makeTextarea("");

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        ""
      );
    });
    expect(setValue).toHaveBeenCalledWith("only");
    setValue.mockClear();

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", makeTextarea("only")),
        "only"
      );
    });
    expect(setValue).not.toHaveBeenCalled();
  });

  it("deduplicates entries", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("dup");
      result.current.addEntry("other");
      result.current.addEntry("dup");
    });

    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY)!);
    expect(stored).toEqual(["other", "dup"]);
  });

  it("persists across hook instances via localStorage", () => {
    const setValue1 = vi.fn();
    const { result: r1 } = renderHook(() =>
      useInputHistory({ setValue: setValue1 })
    );

    act(() => {
      r1.current.addEntry("persistent");
    });

    const setValue2 = vi.fn();
    const { result: r2 } = renderHook(() =>
      useInputHistory({ setValue: setValue2 })
    );

    const textarea = makeTextarea("");
    act(() => {
      r2.current.handleHistoryKeyDown(makeKeyEvent("ArrowUp", textarea), "");
    });
    expect(setValue2).toHaveBeenCalledWith("persistent");
  });

  it("only navigates up when cursor is on the first line", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("msg");
    });

    const textarea = makeTextarea("line1\nline2", 8);
    let handled: boolean;
    act(() => {
      handled = result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        "line1\nline2"
      );
    });
    expect(handled!).toBe(false);
    expect(setValue).not.toHaveBeenCalled();
  });

  it("only navigates down when cursor is on the last line", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("msg");
    });

    const textarea = makeTextarea("");
    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        ""
      );
    });
    expect(setValue).toHaveBeenCalledWith("msg");
    setValue.mockClear();

    const multiline = makeTextarea("line1\nline2", 2);
    let handled: boolean;
    act(() => {
      handled = result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowDown", multiline),
        "line1\nline2"
      );
    });
    expect(handled!).toBe(false);
    expect(setValue).not.toHaveBeenCalled();
  });

  it("resetNavigation returns to draft mode", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("msg");
    });

    const textarea = makeTextarea("draft");
    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", textarea),
        "draft"
      );
    });
    expect(setValue).toHaveBeenCalledWith("msg");
    setValue.mockClear();

    act(() => {
      result.current.resetNavigation();
    });

    act(() => {
      result.current.handleHistoryKeyDown(
        makeKeyEvent("ArrowUp", makeTextarea("")),
        ""
      );
    });
    expect(setValue).toHaveBeenCalledWith("msg");
  });

  it("caps entries at 50", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      for (let i = 0; i < 60; i++) {
        result.current.addEntry(`msg-${i}`);
      }
    });

    const stored: string[] = JSON.parse(localStorage.getItem(STORAGE_KEY)!);
    expect(stored.length).toBe(50);
    expect(stored[0]).toBe("msg-10");
    expect(stored[49]).toBe("msg-59");
  });

  it("ignores whitespace-only entries", () => {
    const setValue = vi.fn();
    const { result } = renderHook(() => useInputHistory({ setValue }));

    act(() => {
      result.current.addEntry("   ");
      result.current.addEntry("\n\t");
    });

    const stored = localStorage.getItem(STORAGE_KEY);
    expect(JSON.parse(stored!)).toEqual([]);
  });
});
