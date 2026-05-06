import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SessionFilter } from "./SessionFilter";
import { useIdeationSessions } from "@/hooks/useIdeation";
import { useProjectStore } from "@/stores/projectStore";
import type { IdeationSessionResponse } from "@/api/ideation.types";

vi.mock("@/hooks/useIdeation", () => ({
  useIdeationSessions: vi.fn(),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn(),
}));

function makeSession(overrides: Partial<IdeationSessionResponse> = {}): IdeationSessionResponse {
  return {
    id: "session-1",
    projectId: "project-1",
    title: "First session",
    titleSource: "user",
    status: "active",
    planArtifactId: null,
    seedTaskId: null,
    parentSessionId: null,
    teamMode: null,
    teamConfig: null,
    createdAt: "2026-05-01T10:00:00+00:00",
    updatedAt: "2026-05-01T10:00:00+00:00",
    archivedAt: null,
    convertedAt: null,
    verificationStatus: "not_started",
    verificationInProgress: false,
    gapScore: null,
    sessionPurpose: "general",
    acceptanceStatus: null,
    ...overrides,
  } as IdeationSessionResponse;
}

const mockedUseIdeationSessions = vi.mocked(useIdeationSessions);
const mockedUseProjectStore = vi.mocked(useProjectStore);

beforeEach(() => {
  mockedUseProjectStore.mockImplementation((selector: unknown) => {
    if (typeof selector === "function") {
      return (selector as (s: { activeProjectId: string | null }) => unknown)({
        activeProjectId: "project-1",
      });
    }
    return undefined;
  });
});

describe("SessionFilter", () => {
  it("renders empty trigger label when no session selected", () => {
    mockedUseIdeationSessions.mockReturnValue({
      data: undefined,
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Session/i })).toBeInTheDocument();
  });

  it("shows loading state in popover", async () => {
    mockedUseIdeationSessions.mockReturnValue({
      data: undefined,
      isLoading: true,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: /Session/i }));
    expect(screen.getByText(/Loading sessions/i)).toBeInTheDocument();
  });

  it("shows empty state with no sessions", async () => {
    mockedUseIdeationSessions.mockReturnValue({
      data: [],
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: /Session/i }));
    expect(screen.getByText(/No sessions available/i)).toBeInTheDocument();
  });

  it("filters sessions by title via search and shows no-match state", async () => {
    const sessions = [
      makeSession({ id: "s-alpha", title: "Alpha thoughts", updatedAt: "2026-05-03T10:00:00+00:00" }),
      makeSession({ id: "s-beta", title: "Beta planning", updatedAt: "2026-05-02T10:00:00+00:00" }),
    ];
    mockedUseIdeationSessions.mockReturnValue({
      data: sessions,
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Session/i }));

    expect(screen.getByText("Alpha thoughts")).toBeInTheDocument();
    expect(screen.getByText("Beta planning")).toBeInTheDocument();

    const searchInput = screen.getByPlaceholderText(/Search sessions/i);
    await user.type(searchInput, "alpha");

    expect(screen.getByText("Alpha thoughts")).toBeInTheDocument();
    expect(screen.queryByText("Beta planning")).toBeNull();

    await user.clear(searchInput);
    await user.type(searchInput, "zzz-no-match");
    expect(screen.getByText(/No sessions match your search/i)).toBeInTheDocument();
  });

  it("excludes archived sessions and limits to 15 most recent", async () => {
    const archived = makeSession({ id: "arch", title: "Archived one", archivedAt: "2026-05-04T00:00:00+00:00" });
    const many: IdeationSessionResponse[] = Array.from({ length: 20 }).map((_, i) =>
      makeSession({
        id: `s-${i}`,
        title: `Session number ${i}`,
        updatedAt: `2026-04-${String(i + 1).padStart(2, "0")}T10:00:00+00:00`,
      }),
    );
    mockedUseIdeationSessions.mockReturnValue({
      data: [archived, ...many],
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Session/i }));

    expect(screen.queryByText("Archived one")).toBeNull();
    // Most recent (index 19) should be visible; oldest (index 0..4) should be sliced out.
    expect(screen.getByText("Session number 19")).toBeInTheDocument();
    expect(screen.queryByText("Session number 0")).toBeNull();
  });

  it("calls onChange when selecting a session and closes popover", async () => {
    const onChange = vi.fn();
    const sessions = [makeSession({ id: "s-x", title: "Pick me" })];
    mockedUseIdeationSessions.mockReturnValue({
      data: sessions,
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId={null} onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: /Session/i }));
    await user.click(screen.getByText("Pick me"));
    expect(onChange).toHaveBeenCalledWith("s-x");
  });

  it("shows selected session title and clears via badge click", async () => {
    const onChange = vi.fn();
    const selected = makeSession({ id: "s-sel", title: "Selected title" });
    mockedUseIdeationSessions.mockReturnValue({
      data: [selected],
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    const { container } = render(
      <SessionFilter selectedSessionId="s-sel" onChange={onChange} />,
    );
    expect(screen.getByText("Selected title")).toBeInTheDocument();

    // Find clear badge (X icon wrapper) — sibling span on the trigger button.
    const xBadge = container.querySelector("span.cursor-pointer");
    expect(xBadge).not.toBeNull();
    await user.click(xBadge!);
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("falls back to truncated session id when title missing", async () => {
    const session = makeSession({
      id: "abcdef1234567890",
      title: null,
    });
    mockedUseIdeationSessions.mockReturnValue({
      data: [session],
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId="abcdef1234567890" onChange={vi.fn()} />);
    expect(screen.getByText(/Session abcdef12\.\.\./)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Session abcdef12/i }));
    // List item also shows the same fallback.
    const matches = screen.getAllByText(/Session abcdef12\.\.\./);
    expect(matches.length).toBeGreaterThan(0);
  });

  it("filters sessions by id substring", async () => {
    const sessions = [
      makeSession({ id: "uuid-zzz-001", title: "Hello" }),
      makeSession({ id: "uuid-yyy-002", title: "Other" }),
    ];
    mockedUseIdeationSessions.mockReturnValue({
      data: sessions,
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Session/i }));
    await user.type(screen.getByPlaceholderText(/Search sessions/i), "zzz");
    expect(screen.getByText("Hello")).toBeInTheDocument();
    expect(screen.queryByText("Other")).toBeNull();
  });

  it("clears search when popover closes", async () => {
    const sessions = [makeSession({ id: "s-1", title: "Visible" })];
    mockedUseIdeationSessions.mockReturnValue({
      data: sessions,
      isLoading: false,
    } as unknown as ReturnType<typeof useIdeationSessions>);

    const user = userEvent.setup();
    render(<SessionFilter selectedSessionId={null} onChange={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: /Session/i });
    await user.click(trigger);
    await user.type(screen.getByPlaceholderText(/Search sessions/i), "Visible");
    // Close
    await user.click(trigger);
    // Re-open and search input should be empty
    await user.click(trigger);
    const reopened = screen.getByPlaceholderText(/Search sessions/i) as HTMLInputElement;
    expect(reopened.value).toBe("");
  });
});
