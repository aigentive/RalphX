import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { UpdateChannel } from "./update-channel.schemas";

const ReleaseNotesResponseSchema = z.object({
  version: z.string(),
  body: z.string().nullable(),
  source: z.enum(["bundled_resource", "development_checkout", "missing"]),
});

export type ReleaseNotesResponse = z.infer<typeof ReleaseNotesResponseSchema>;

const GITHUB_RELEASES_URL =
  "https://api.github.com/repos/aigentive/ralphx.app/releases";
const GITHUB_RELEASE_PAGE_SIZE = 100;
const MAX_GITHUB_RELEASE_PAGES = 10;

export interface ReleaseMetadata {
  version: string;
  publishedAt: string;
  name: string | null;
  body: string | null;
  prerelease: boolean;
}

export type ReleaseMetadataAvailability = "available" | "stale" | "unavailable";

export class ReleaseMetadataSnapshot extends Map<string, ReleaseMetadata> {
  constructor(
    entries: Iterable<readonly [string, ReleaseMetadata]> = [],
    readonly availability: ReleaseMetadataAvailability = "available",
  ) {
    super(entries);
  }
}

const GitHubReleaseSchema = z.object({
  tag_name: z.string(),
  published_at: z.string().nullable(),
  name: z.string().nullable(),
  body: z.string().nullable(),
  draft: z.boolean(),
  prerelease: z.boolean(),
});

const CACHE_KEY = "ralphx-release-metadata";
const CACHE_TTL_MS = 15 * 60 * 1000;
const NUMERIC_VERSION_TAG = /^v?(\d+\.\d+\.\d+)$/;

const ReleaseMetadataSchema = z.object({
  version: z.string(),
  publishedAt: z.string(),
  name: z.string().nullable(),
  body: z.string().nullable(),
  prerelease: z.boolean(),
});

const CacheEntrySchema = z.object({
  fetchedAt: z.number(),
  releases: z.array(ReleaseMetadataSchema),
});

interface MemoryCacheEntry {
  fetchedAt: number;
  releases: ReleaseMetadataSnapshot;
}

let memoryCache: MemoryCacheEntry | null = null;

function isFresh(fetchedAt: number): boolean {
  return Date.now() - fetchedAt < CACHE_TTL_MS;
}

function loadFromStorage(): MemoryCacheEntry | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = CacheEntrySchema.safeParse(JSON.parse(raw));
    if (!parsed.success) {
      localStorage.removeItem(CACHE_KEY);
      return null;
    }
    const map = new ReleaseMetadataSnapshot();
    for (const r of parsed.data.releases) {
      map.set(r.version, r);
    }
    return { fetchedAt: parsed.data.fetchedAt, releases: map };
  } catch {
    localStorage.removeItem(CACHE_KEY);
    return null;
  }
}

function saveToStorage(entry: MemoryCacheEntry): void {
  try {
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({
        fetchedAt: entry.fetchedAt,
        releases: Array.from(entry.releases.values()),
      }),
    );
  } catch {
    // localStorage full or unavailable
  }
}

function releasePageUrl(page: number): string {
  const url = new URL(GITHUB_RELEASES_URL);
  url.searchParams.set("per_page", String(GITHUB_RELEASE_PAGE_SIZE));
  url.searchParams.set("page", String(page));
  return url.toString();
}

async function fetchAllGitHubReleases(): Promise<
  z.infer<typeof GitHubReleaseSchema>[]
> {
  const allReleases: z.infer<typeof GitHubReleaseSchema>[] = [];
  for (let page = 1; page <= MAX_GITHUB_RELEASE_PAGES; page += 1) {
    const response = await fetch(releasePageUrl(page), {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) {
      throw new Error(`GitHub releases request failed with ${response.status}`);
    }
    const pageReleases = z.array(GitHubReleaseSchema).parse(await response.json());
    allReleases.push(...pageReleases);
    if (pageReleases.length < GITHUB_RELEASE_PAGE_SIZE) {
      return allReleases;
    }
  }
  throw new Error(
    `GitHub release history exceeded ${MAX_GITHUB_RELEASE_PAGES} pages`,
  );
}

function withAvailability(
  cache: MemoryCacheEntry,
  availability: ReleaseMetadataAvailability,
): ReleaseMetadataSnapshot {
  if (cache.releases.availability === availability) {
    return cache.releases;
  }
  return new ReleaseMetadataSnapshot(cache.releases, availability);
}

export async function fetchReleaseMetadata(): Promise<ReleaseMetadataSnapshot> {
  if (memoryCache && isFresh(memoryCache.fetchedAt)) {
    return memoryCache.releases;
  }

  const stored = loadFromStorage();
  const lastKnownGood =
    memoryCache === null ||
    (stored !== null && stored.fetchedAt > memoryCache.fetchedAt)
      ? stored
      : memoryCache;
  if (lastKnownGood && isFresh(lastKnownGood.fetchedAt)) {
    memoryCache = lastKnownGood;
    return lastKnownGood.releases;
  }

  try {
    const releases = await fetchAllGitHubReleases();

    const map = new ReleaseMetadataSnapshot();
    for (const r of releases) {
      const versionMatch = NUMERIC_VERSION_TAG.exec(r.tag_name);
      if (!versionMatch || !r.published_at || r.draft) continue;
      const version = versionMatch[1];
      if (version === undefined) continue;
      map.set(version, {
        version,
        publishedAt: r.published_at,
        name: r.name,
        body: r.body,
        prerelease: r.prerelease,
      });
    }
    memoryCache = { fetchedAt: Date.now(), releases: map };
    saveToStorage(memoryCache);
    return map;
  } catch {
    if (lastKnownGood) {
      memoryCache = lastKnownGood;
      return withAvailability(lastKnownGood, "stale");
    }
    memoryCache = null;
    return new ReleaseMetadataSnapshot([], "unavailable");
  }
}

export async function getCurrentReleaseNotes(): Promise<ReleaseNotesResponse> {
  const response = await invoke<unknown>("get_current_release_notes");
  return ReleaseNotesResponseSchema.parse(response);
}

export async function getLastSeenReleaseNotesVersion(): Promise<string | null> {
  return invoke<string | null>("get_last_seen_release_notes_version");
}

export async function markReleaseNotesSeen(version: string): Promise<void> {
  await invoke("mark_release_notes_seen", { version });
}

export async function listReleaseNotesVersions(): Promise<string[]> {
  const response = await invoke<unknown>("list_release_notes_versions");
  return z.array(z.string()).parse(response);
}

export async function getReleaseNotesForVersion(
  version: string,
): Promise<ReleaseNotesResponse> {
  const response = await invoke<unknown>("get_release_notes_for_version", {
    version,
  });
  return ReleaseNotesResponseSchema.parse(response);
}

export function compareSemverDesc(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] ?? 0;
    const nb = pb[i] ?? 0;
    if (nb !== na) return nb - na;
  }
  return 0;
}

export function mergeVersionLists(
  bundledVersions: string[],
  metadata: Map<string, ReleaseMetadata>,
  channel: UpdateChannel,
): string[] {
  const versionSet = new Set<string>();
  if (channel === "stable") {
    for (const version of bundledVersions) {
      if (metadata.get(version)?.prerelease !== true) {
        versionSet.add(version);
      }
    }
  }
  for (const [version, release] of metadata) {
    if (release.prerelease === (channel === "nightly")) {
      versionSet.add(version);
    }
  }
  return Array.from(versionSet).sort(compareSemverDesc);
}
