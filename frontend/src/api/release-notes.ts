import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const ReleaseNotesResponseSchema = z.object({
  version: z.string(),
  body: z.string().nullable(),
  source: z.enum(["bundled_resource", "development_checkout", "missing"]),
});

export type ReleaseNotesResponse = z.infer<typeof ReleaseNotesResponseSchema>;

const GITHUB_RELEASES_URL =
  "https://api.github.com/repos/aigentive/ralphx.app/releases?per_page=100";

export interface ReleaseMetadata {
  version: string;
  publishedAt: string;
  name: string | null;
  body: string | null;
}

const GitHubReleaseSchema = z.object({
  tag_name: z.string(),
  published_at: z.string().nullable(),
  name: z.string().nullable(),
  body: z.string().nullable(),
});

const CACHE_KEY = "ralphx-release-metadata";
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

interface CacheEntry {
  fetchedAt: number;
  releases: Array<{
    version: string;
    publishedAt: string;
    name: string | null;
    body: string | null;
  }>;
}

let memoryCache: Map<string, ReleaseMetadata> | null = null;

function loadFromStorage(): Map<string, ReleaseMetadata> | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const entry = JSON.parse(raw) as CacheEntry;
    if (Date.now() - entry.fetchedAt > CACHE_TTL_MS) return null;
    const map = new Map<string, ReleaseMetadata>();
    for (const r of entry.releases) {
      map.set(r.version, r);
    }
    return map;
  } catch {
    return null;
  }
}

function saveToStorage(map: Map<string, ReleaseMetadata>): void {
  try {
    const entry: CacheEntry = {
      fetchedAt: Date.now(),
      releases: Array.from(map.values()),
    };
    localStorage.setItem(CACHE_KEY, JSON.stringify(entry));
  } catch {
    // localStorage full or unavailable
  }
}

export async function fetchReleaseMetadata(): Promise<
  Map<string, ReleaseMetadata>
> {
  if (memoryCache) return memoryCache;

  const stored = loadFromStorage();
  if (stored) {
    memoryCache = stored;
    return stored;
  }

  try {
    const resp = await fetch(GITHUB_RELEASES_URL, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!resp.ok) return new Map();

    const json: unknown = await resp.json();
    const releases = z.array(GitHubReleaseSchema).parse(json);

    const map = new Map<string, ReleaseMetadata>();
    for (const r of releases) {
      const version = r.tag_name.replace(/^v/, "");
      if (r.published_at) {
        map.set(version, {
          version,
          publishedAt: r.published_at,
          name: r.name,
          body: r.body,
        });
      }
    }
    memoryCache = map;
    saveToStorage(map);
    return map;
  } catch {
    return new Map();
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
): string[] {
  const versionSet = new Set(bundledVersions);
  for (const version of metadata.keys()) {
    versionSet.add(version);
  }
  return Array.from(versionSet).sort(compareSemverDesc);
}
