use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use tauri::State;

use super::root::{resolve_composer_root, validate_composer_root};
use super::types::{
    AgentComposerEntryResponse, SearchAgentComposerEntriesInput, SearchAgentComposerEntriesResponse,
};
use crate::application::AppState;
use crate::domain::entities::ProjectId;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

const MAX_PROJECT_ENTRY_INDEX: usize = 25_000;
const MAX_PROJECT_ENTRY_LIMIT: usize = 200;
const DEFAULT_PROJECT_ENTRY_LIMIT: usize = 80;
const MAX_QUERY_LEN: usize = 256;
const IGNORED_ENTRY_DIRS: &[&str] = &[
    ".git",
    ".claude",
    ".artifacts",
    ".next",
    ".turbo",
    "build",
    "dist",
    "node_modules",
    "out",
    "target",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedEntry {
    path: String,
    kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    File,
    Directory,
}

#[tauri::command]
pub async fn search_agent_composer_entries(
    input: SearchAgentComposerEntriesInput,
    state: State<'_, AppState>,
) -> Result<SearchAgentComposerEntriesResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id))?;
    let root = resolve_composer_root(&project, input.conversation_id.as_deref(), &state).await?;
    let query = normalize_query(&input.query);
    let limit = input
        .limit
        .unwrap_or(DEFAULT_PROJECT_ENTRY_LIMIT)
        .min(MAX_PROJECT_ENTRY_LIMIT);
    tokio::task::spawn_blocking(move || search_entries_blocking(&root, &query, limit))
        .await
        .map_err(|error| format!("Project entry search failed: {error}"))?
}

fn search_entries_blocking(
    root: &Path,
    query: &str,
    limit: usize,
) -> Result<SearchAgentComposerEntriesResponse, String> {
    let mut entries = collect_git_entries(root).unwrap_or_else(|| collect_fs_entries(root));
    let truncated_index = entries.len() > MAX_PROJECT_ENTRY_INDEX;
    if truncated_index {
        entries.truncate(MAX_PROJECT_ENTRY_INDEX);
    }

    let normalized_query = normalize_query(query);
    let mut scored = entries
        .into_iter()
        .filter_map(|entry| score_entry(&entry, &normalized_query).map(|score| (score, entry)))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.path.len().cmp(&right.path.len()))
            .then_with(|| left.path.cmp(&right.path))
    });
    let total = scored.len();
    let entries = scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry_response(entry))
        .collect::<Vec<_>>();
    Ok(SearchAgentComposerEntriesResponse {
        entries,
        truncated: truncated_index || total > limit,
    })
}

fn collect_git_entries(root: &Path) -> Option<Vec<IndexedEntry>> {
    let safe_root = validate_composer_root(root).ok()?;
    let mut command = Command::new(resolve_git_cli_path());
    command.args(["ls-files", "-co", "--exclude-standard", "-z"]);
    // codeql[rust/path-injection]
    command.current_dir(&safe_root);
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut entries = BTreeMap::<String, EntryKind>::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(raw).replace('\\', "/");
        if path.is_empty() || ignored_relative_path(&path) {
            continue;
        }
        entries.insert(path.clone(), EntryKind::File);
        add_directory_ancestors(&path, &mut entries);
        if entries.len() > MAX_PROJECT_ENTRY_INDEX {
            break;
        }
    }
    Some(indexed_from_map(entries))
}

fn collect_fs_entries(root: &Path) -> Vec<IndexedEntry> {
    let Ok(safe_root) = validate_composer_root(root) else {
        return Vec::new();
    };
    let mut entries = BTreeMap::<String, EntryKind>::new();
    let mut stack = vec![safe_root.clone()];
    while let Some(dir) = stack.pop() {
        if entries.len() > MAX_PROJECT_ENTRY_INDEX {
            break;
        }
        // codeql[rust/path-injection]
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            if entries.len() > MAX_PROJECT_ENTRY_INDEX {
                break;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if file_type.is_dir() && IGNORED_ENTRY_DIRS.contains(&name) {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&safe_root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if relative.is_empty() || ignored_relative_path(&relative) {
                continue;
            }
            if file_type.is_dir() {
                entries.insert(relative, EntryKind::Directory);
                stack.push(path);
            } else if file_type.is_file() {
                entries.insert(relative.clone(), EntryKind::File);
                add_directory_ancestors(&relative, &mut entries);
            }
        }
    }
    indexed_from_map(entries)
}

fn add_directory_ancestors(path: &str, entries: &mut BTreeMap<String, EntryKind>) {
    let mut parts = path.split('/').collect::<Vec<_>>();
    parts.pop();
    let mut current = String::new();
    for part in parts {
        if current.is_empty() {
            current.push_str(part);
        } else {
            current.push('/');
            current.push_str(part);
        }
        if !current.is_empty() {
            entries
                .entry(current.clone())
                .or_insert(EntryKind::Directory);
        }
    }
}

fn indexed_from_map(entries: BTreeMap<String, EntryKind>) -> Vec<IndexedEntry> {
    entries
        .into_iter()
        .map(|(path, kind)| IndexedEntry { path, kind })
        .collect()
}

fn ignored_relative_path(path: &str) -> bool {
    path.split('/')
        .next()
        .is_some_and(|part| IGNORED_ENTRY_DIRS.contains(&part))
}

fn entry_response(entry: IndexedEntry) -> AgentComposerEntryResponse {
    let parent_path = entry
        .path
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .filter(|parent| !parent.is_empty());
    AgentComposerEntryResponse {
        path: entry.path,
        kind: match entry.kind {
            EntryKind::File => "file".to_string(),
            EntryKind::Directory => "directory".to_string(),
        },
        parent_path,
    }
}

fn score_entry(entry: &IndexedEntry, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(1);
    }
    let path = entry.path.to_ascii_lowercase();
    let basename = entry
        .path
        .rsplit('/')
        .next()
        .unwrap_or(entry.path.as_str())
        .to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if basename == query || path == query {
        return Some(1000);
    }
    if basename.starts_with(&query) {
        return Some(850);
    }
    if path.starts_with(&query) {
        return Some(760);
    }
    if path.split('/').any(|part| part.starts_with(&query)) {
        return Some(650);
    }
    if basename.contains(&query) {
        return Some(540);
    }
    if path.contains(&query) {
        return Some(420);
    }
    fuzzy_match_score(&path, &query).map(|score| 220 + score)
}

fn normalize_query(raw: &str) -> String {
    raw.trim()
        .trim_start_matches(['@', '/', '.'])
        .chars()
        .take(MAX_QUERY_LEN)
        .collect::<String>()
        .to_ascii_lowercase()
}

fn fuzzy_match_score(text: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let mut score = 0;
    let mut search_from = 0usize;
    for ch in query.chars() {
        let haystack = &text[search_from..];
        let index = haystack.find(ch)?;
        score += if index == 0 { 12 } else { 5 };
        search_from += index + ch.len_utf8();
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn search_entries_ranks_basename_prefix_over_path_contains() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/components")).expect("dirs");
        fs::write(temp.path().join("src/components/Composer.tsx"), "").expect("write");
        fs::write(temp.path().join("src/other.ts"), "").expect("write");

        let result = search_entries_blocking(temp.path(), "comp", 10).expect("search");

        assert_eq!(result.entries[0].path, "src/components");
        assert!(result
            .entries
            .iter()
            .any(|entry| entry.path == "src/components/Composer.tsx"));
    }

    #[test]
    fn normalize_query_trims_composer_prefixes() {
        assert_eq!(normalize_query(" @src "), "src");
        assert_eq!(normalize_query("./src"), "src");
    }

    #[test]
    fn empty_query_returns_initial_bounded_entries() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("dirs");
        fs::write(temp.path().join("src/main.ts"), "").expect("write");

        let result = search_entries_blocking(temp.path(), "", 10).expect("search");

        assert!(result
            .entries
            .iter()
            .any(|entry| entry.path == "src/main.ts"));
    }

    #[test]
    fn filesystem_index_ignores_heavy_dirs_and_records_parent_paths() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/components")).expect("src dirs");
        fs::create_dir_all(temp.path().join("node_modules/pkg")).expect("ignored dir");
        fs::write(temp.path().join("src/components/Button.tsx"), "").expect("src file");
        fs::write(temp.path().join("node_modules/pkg/index.js"), "").expect("ignored file");

        let entries = collect_fs_entries(temp.path());

        assert!(entries
            .iter()
            .any(|entry| entry.path == "src" && entry.kind == EntryKind::Directory));
        assert!(entries.iter().any(|entry| {
            entry.path == "src/components/Button.tsx" && entry.kind == EntryKind::File
        }));
        assert!(!entries
            .iter()
            .any(|entry| entry.path.starts_with("node_modules")));

        let response = entry_response(IndexedEntry {
            path: "src/components/Button.tsx".to_string(),
            kind: EntryKind::File,
        });
        assert_eq!(response.parent_path.as_deref(), Some("src/components"));
        assert_eq!(response.kind, "file");
    }

    #[test]
    fn search_entries_reports_truncation_when_limit_is_smaller_than_matches() {
        let temp = tempdir().expect("tempdir");
        fs::write(temp.path().join("alpha.ts"), "").expect("alpha");
        fs::write(temp.path().join("beta.ts"), "").expect("beta");

        let result = search_entries_blocking(temp.path(), "", 1).expect("search");

        assert_eq!(result.entries.len(), 1);
        assert!(result.truncated);
    }

    #[test]
    fn score_entry_covers_match_tiers_and_fuzzy_misses() {
        let entry = IndexedEntry {
            path: "src/components/AgentComposerSurface.tsx".to_string(),
            kind: EntryKind::File,
        };

        assert_eq!(score_entry(&entry, "AgentComposerSurface.tsx"), Some(1000));
        assert_eq!(score_entry(&entry, "agent"), Some(850));
        assert_eq!(score_entry(&entry, "src"), Some(760));
        assert_eq!(score_entry(&entry, "comp"), Some(650));
        assert_eq!(score_entry(&entry, "poser"), Some(540));
        assert_eq!(score_entry(&entry, "components/agent"), Some(420));
        assert!(score_entry(&entry, "acs").is_some_and(|score| score >= 220));
        assert_eq!(score_entry(&entry, "zzzz"), None);
    }

    #[test]
    fn ignored_relative_path_checks_only_top_level_directory() {
        assert!(ignored_relative_path("target/debug/app"));
        assert!(!ignored_relative_path("src/target/mod.rs"));
        assert_eq!(fuzzy_match_score("composer", "cmr"), Some(22));
        assert_eq!(fuzzy_match_score("composer", "xyz"), None);
    }
}
