import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const CACHE_KEY = "ralphx-release-metadata";

describe("release-notes", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
    vi.resetModules();
  });

  describe("compareSemverDesc", () => {
    it("sorts newer versions first (descending)", async () => {
      const { compareSemverDesc } = await import("./release-notes");
      expect(compareSemverDesc("2.0.0", "1.0.0")).toBeLessThan(0);
      expect(compareSemverDesc("1.0.0", "2.0.0")).toBeGreaterThan(0);
    });

    it("returns 0 for equal versions", async () => {
      const { compareSemverDesc } = await import("./release-notes");
      expect(compareSemverDesc("1.2.3", "1.2.3")).toBe(0);
    });

    it("compares minor and patch correctly", async () => {
      const { compareSemverDesc } = await import("./release-notes");
      expect(compareSemverDesc("0.10.0", "0.9.0")).toBeLessThan(0);
      expect(compareSemverDesc("1.0.2", "1.0.1")).toBeLessThan(0);
    });

    it("handles different length version strings", async () => {
      const { compareSemverDesc } = await import("./release-notes");
      expect(compareSemverDesc("1.0", "1.0.0")).toBe(0);
      expect(compareSemverDesc("1.0.0.1", "1.0.0")).toBeLessThan(0);
    });
  });

  describe("mergeVersionLists", () => {
    it("merges bundled and GitHub versions, deduplicates, sorts descending", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      const meta = new Map([
        ["2.0.0", { version: "2.0.0", publishedAt: "2024-06-01T00:00:00Z", name: null, body: null }],
        ["1.0.0", { version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null }],
      ]);
      const result = mergeVersionLists(["1.0.0", "0.9.0"], meta);
      expect(result).toEqual(["2.0.0", "1.0.0", "0.9.0"]);
    });

    it("handles empty inputs", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      expect(mergeVersionLists([], new Map())).toEqual([]);
    });

    it("handles bundled-only versions", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      expect(mergeVersionLists(["1.0.0", "0.9.0"], new Map())).toEqual(["1.0.0", "0.9.0"]);
    });
  });

  describe("fetchReleaseMetadata", () => {
    it("fetches from GitHub API and caches to localStorage", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      const apiResponse = [
        { tag_name: "v1.2.0", published_at: "2024-06-01T00:00:00Z", name: "Release 1.2.0", body: "Release notes body" },
        { tag_name: "v1.1.0", published_at: "2024-05-01T00:00:00Z", name: null, body: null },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const result = await fetchReleaseMetadata();

      expect(result.size).toBe(2);
      expect(result.get("1.2.0")).toMatchObject({
        version: "1.2.0",
        publishedAt: "2024-06-01T00:00:00Z",
        name: "Release 1.2.0",
        body: "Release notes body",
      });
      expect(result.get("1.1.0")).toMatchObject({ version: "1.1.0", body: null });

      const cached = localStorage.getItem(CACHE_KEY);
      expect(cached).toBeTruthy();
      const parsed = JSON.parse(cached!) as { fetchedAt: number; releases: unknown[] };
      expect(parsed.releases).toHaveLength(2);
    });

    it("returns empty map on fetch failure", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 500 }));

      const result = await fetchReleaseMetadata();
      expect(result.size).toBe(0);
    });

    it("returns empty map on network error", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("network")));

      const result = await fetchReleaseMetadata();
      expect(result.size).toBe(0);
    });

    it("loads from localStorage cache when valid", async () => {
      const cached = {
        fetchedAt: Date.now(),
        releases: [{ version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: "cached body" }],
      };
      localStorage.setItem(CACHE_KEY, JSON.stringify(cached));

      const fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(fetchSpy).not.toHaveBeenCalled();
      expect(result.size).toBe(1);
      expect(result.get("1.0.0")?.body).toBe("cached body");
    });

    it("ignores expired localStorage cache", async () => {
      const cached = {
        fetchedAt: Date.now() - 25 * 60 * 60 * 1000,
        releases: [{ version: "old", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null }],
      };
      localStorage.setItem(CACHE_KEY, JSON.stringify(cached));

      const apiResponse = [
        { tag_name: "v2.0.0", published_at: "2024-06-01T00:00:00Z", name: null, body: null },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.has("2.0.0")).toBe(true);
      expect(result.has("old")).toBe(false);
    });

    it("skips releases without published_at", async () => {
      const apiResponse = [
        { tag_name: "v1.0.0", published_at: null, name: null, body: null },
        { tag_name: "v0.9.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.size).toBe(1);
      expect(result.has("0.9.0")).toBe(true);
    });

    it("returns memory cache on second call", async () => {
      const apiResponse = [
        { tag_name: "v1.0.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null },
      ];
      const fetchSpy = vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      const first = await fetchReleaseMetadata();
      const second = await fetchReleaseMetadata();

      expect(fetchSpy).toHaveBeenCalledTimes(1);
      expect(first).toBe(second);
    });

    it("strips v prefix from tag names", async () => {
      const apiResponse = [
        { tag_name: "v0.10.32", published_at: "2024-01-01T00:00:00Z", name: null, body: null },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.has("0.10.32")).toBe(true);
    });

    it("handles corrupted localStorage gracefully", async () => {
      localStorage.setItem(CACHE_KEY, "not json");

      const apiResponse = [
        { tag_name: "v1.0.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.size).toBe(1);
    });
  });

  describe("listReleaseNotesVersions", () => {
    it("invokes Tauri command and parses result", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(["1.0.0", "0.9.0"]);

      const { listReleaseNotesVersions } = await import("./release-notes");
      const result = await listReleaseNotesVersions();

      expect(invoke).toHaveBeenCalledWith("list_release_notes_versions");
      expect(result).toEqual(["1.0.0", "0.9.0"]);
    });
  });

  describe("getReleaseNotesForVersion", () => {
    it("invokes Tauri command with version param", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      (invoke as ReturnType<typeof vi.fn>).mockResolvedValue({
        version: "1.0.0",
        body: "notes",
        source: "bundled_resource",
      });

      const { getReleaseNotesForVersion } = await import("./release-notes");
      const result = await getReleaseNotesForVersion("1.0.0");

      expect(invoke).toHaveBeenCalledWith("get_release_notes_for_version", { version: "1.0.0" });
      expect(result.body).toBe("notes");
    });
  });
});
