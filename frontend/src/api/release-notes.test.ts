import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

  afterEach(() => {
    vi.useRealTimers();
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
    it("merges bundled and Stable GitHub versions, deduplicates, sorts descending", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      const meta = new Map([
        ["2.0.0", { version: "2.0.0", publishedAt: "2024-06-01T00:00:00Z", name: null, body: null, prerelease: false }],
        ["1.0.0", { version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null, prerelease: false }],
      ]);
      const result = mergeVersionLists(["1.0.0", "0.9.0"], meta, "stable");
      expect(result).toEqual(["2.0.0", "1.0.0", "0.9.0"]);
    });

    it("handles empty inputs", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      expect(mergeVersionLists([], new Map(), "stable")).toEqual([]);
    });

    it("treats bundled-only legacy versions as Stable", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      expect(mergeVersionLists(["1.0.0", "0.9.0"], new Map(), "stable")).toEqual(["1.0.0", "0.9.0"]);
      expect(mergeVersionLists(["1.0.0", "0.9.0"], new Map(), "nightly")).toEqual([]);
    });

    it("keeps metadata-classified Nightly versions out of Stable history", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      const meta = new Map([
        ["2.0.0", { version: "2.0.0", publishedAt: "2024-06-01T00:00:00Z", name: null, body: null, prerelease: true }],
        ["1.0.0", { version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null, prerelease: false }],
      ]);

      expect(mergeVersionLists(["2.0.0", "1.0.0", "0.9.0"], meta, "stable")).toEqual([
        "1.0.0",
        "0.9.0",
      ]);
      expect(mergeVersionLists(["2.0.0", "1.0.0", "0.9.0"], meta, "nightly")).toEqual([
        "2.0.0",
      ]);
    });

    it("moves a promoted version from Nightly to Stable without duplicating history", async () => {
      const { mergeVersionLists } = await import("./release-notes");
      const promoted = new Map([
        ["2.0.0", { version: "2.0.0", publishedAt: "2024-06-01T00:00:00Z", name: null, body: null, prerelease: false }],
        ["1.0.0", { version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null, prerelease: false }],
      ]);

      expect(mergeVersionLists(["1.0.0"], promoted, "stable")).toEqual(["2.0.0", "1.0.0"]);
      expect(mergeVersionLists(["1.0.0"], promoted, "nightly")).toEqual([]);
    });
  });

  describe("fetchReleaseMetadata", () => {
    it("fetches from GitHub API and caches to localStorage", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      const apiResponse = [
        { tag_name: "v1.2.0", published_at: "2024-06-01T00:00:00Z", name: "Release 1.2.0", body: "Release notes body", draft: false, prerelease: false },
        { tag_name: "v1.1.0", published_at: "2024-05-01T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
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
        prerelease: false,
      });
      expect(result.get("1.1.0")).toMatchObject({ version: "1.1.0", body: null, prerelease: true });

      const cached = localStorage.getItem(CACHE_KEY);
      expect(cached).toBeTruthy();
      const parsed = JSON.parse(cached!) as { fetchedAt: number; releases: unknown[] };
      expect(parsed.releases).toHaveLength(2);
    });

    it("reports classification unavailable on a non-ok response without cache", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 500 }));

      const result = await fetchReleaseMetadata();
      expect(result.size).toBe(0);
      expect(result.availability).toBe("unavailable");
    });

    it("reports classification unavailable on a network error without cache", async () => {
      const { fetchReleaseMetadata } = await import("./release-notes");
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("network")));

      const result = await fetchReleaseMetadata();
      expect(result.size).toBe(0);
      expect(result.availability).toBe("unavailable");
    });

    it("loads from localStorage cache when valid", async () => {
      const cached = {
        fetchedAt: Date.now(),
        releases: [{ version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: "cached body", prerelease: false }],
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

    it("refreshes an expired localStorage cache when GitHub succeeds", async () => {
      const cached = {
        fetchedAt: Date.now() - 25 * 60 * 60 * 1000,
        releases: [{ version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null, prerelease: false }],
      };
      localStorage.setItem(CACHE_KEY, JSON.stringify(cached));

      const apiResponse = [
        { tag_name: "v2.0.0", published_at: "2024-06-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.has("2.0.0")).toBe(true);
      expect(result.has("1.0.0")).toBe(false);
    });

    it("skips releases without published_at", async () => {
      const apiResponse = [
        { tag_name: "v1.0.0", published_at: null, name: null, body: null, draft: false, prerelease: false },
        { tag_name: "v0.9.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
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
        { tag_name: "v1.0.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
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
        { tag_name: "v0.10.32", published_at: "2024-01-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
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
        { tag_name: "v1.0.0", published_at: "2024-01-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.size).toBe(1);
    });

    it("accepts only published exact numeric version tags and ignores drafts and updater pointers", async () => {
      const apiResponse = [
        { tag_name: "v1.2.3", published_at: "2024-06-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
        { tag_name: "1.2.2", published_at: "2024-05-01T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
        { tag_name: "updater-stable", published_at: "2024-06-02T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
        { tag_name: "updater-nightly", published_at: "2024-06-02T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
        { tag_name: "v1.2.4-beta.1", published_at: "2024-06-03T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
        { tag_name: "v1.2.5", published_at: "2024-06-04T00:00:00Z", name: null, body: null, draft: true, prerelease: false },
      ];
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(apiResponse),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect([...result.keys()]).toEqual(["1.2.3", "1.2.2"]);
    });

    it("invalidates legacy cached entries without prerelease classification", async () => {
      localStorage.setItem(CACHE_KEY, JSON.stringify({
        fetchedAt: Date.now(),
        releases: [{ version: "1.0.0", publishedAt: "2024-01-01T00:00:00Z", name: null, body: null }],
      }));
      const fetchSpy = vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve([
          { tag_name: "v2.0.0", published_at: "2024-06-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
        ]),
      });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(fetchSpy).toHaveBeenCalledTimes(1);
      expect([...result.keys()]).toEqual(["2.0.0"]);
    });

    it("expires the in-memory classification cache after fifteen minutes", async () => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date("2026-07-23T12:00:00Z"));
      const fetchSpy = vi.fn()
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve([
            { tag_name: "v2.0.0", published_at: "2024-06-01T00:00:00Z", name: null, body: null, draft: false, prerelease: true },
          ]),
        })
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve([
            { tag_name: "v2.0.0", published_at: "2024-06-01T00:00:00Z", name: null, body: null, draft: false, prerelease: false },
          ]),
        });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      expect((await fetchReleaseMetadata()).get("2.0.0")?.prerelease).toBe(true);
      vi.advanceTimersByTime(15 * 60 * 1000 + 1);
      expect((await fetchReleaseMetadata()).get("2.0.0")?.prerelease).toBe(false);
      expect(fetchSpy).toHaveBeenCalledTimes(2);
      vi.useRealTimers();
    });

    it("fetches more than 100 releases across bounded pages", async () => {
      const firstPage = Array.from({ length: 100 }, (_, index) => ({
        tag_name: `v1.0.${index}`,
        published_at: "2026-07-01T00:00:00Z",
        name: null,
        body: null,
        draft: false,
        prerelease: false,
      }));
      const secondPage = [
        {
          tag_name: "v2.0.0",
          published_at: "2026-07-02T00:00:00Z",
          name: null,
          body: null,
          draft: false,
          prerelease: true,
        },
      ];
      const fetchSpy = vi.fn()
        .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(firstPage) })
        .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(secondPage) });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.size).toBe(101);
      expect(result.get("2.0.0")?.prerelease).toBe(true);
      expect(fetchSpy).toHaveBeenNthCalledWith(
        2,
        expect.stringContaining("page=2"),
        expect.any(Object),
      );
    });

    it("stops pagination when a page contains fewer than 100 releases", async () => {
      const fetchSpy = vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve([
          {
            tag_name: "v1.0.0",
            published_at: "2026-07-01T00:00:00Z",
            name: null,
            body: null,
            draft: false,
            prerelease: false,
          },
        ]),
      });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      await fetchReleaseMetadata();

      expect(fetchSpy).toHaveBeenCalledTimes(1);
    });

    it("fails explicitly when the pagination cap is exhausted", async () => {
      const fullPage = Array.from({ length: 100 }, (_, index) => ({
        tag_name: `v1.0.${index}`,
        published_at: "2026-07-01T00:00:00Z",
        name: null,
        body: null,
        draft: false,
        prerelease: false,
      }));
      const fetchSpy = vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(fullPage),
      });
      vi.stubGlobal("fetch", fetchSpy);

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(fetchSpy).toHaveBeenCalledTimes(10);
      expect(result.availability).toBe("unavailable");
      expect(result.size).toBe(0);
    });

    it("retains an expired valid cache when refresh fails", async () => {
      localStorage.setItem(CACHE_KEY, JSON.stringify({
        fetchedAt: Date.now() - 16 * 60 * 1000,
        releases: [{
          version: "1.0.0",
          publishedAt: "2026-07-01T00:00:00Z",
          name: null,
          body: "last known good",
          prerelease: false,
        }],
      }));
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("network")));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.availability).toBe("stale");
      expect(result.get("1.0.0")?.body).toBe("last known good");
      expect(localStorage.getItem(CACHE_KEY)).toContain("last known good");
    });

    it("retains last-known-good metadata after a schema failure", async () => {
      localStorage.setItem(CACHE_KEY, JSON.stringify({
        fetchedAt: Date.now() - 16 * 60 * 1000,
        releases: [{
          version: "1.0.0",
          publishedAt: "2026-07-01T00:00:00Z",
          name: null,
          body: null,
          prerelease: false,
        }],
      }));
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve([{ invalid: true }]),
      }));

      const { fetchReleaseMetadata } = await import("./release-notes");
      const result = await fetchReleaseMetadata();

      expect(result.availability).toBe("stale");
      expect(result.has("1.0.0")).toBe(true);
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
