// Commit message construction helpers for merge workflows.
//
// Pure functions extracted from side_effects.rs for maintainability.
// These build conventional-commit-style messages for squash/plan merges.

use crate::domain::entities::IdeationSessionId;
use crate::domain::entities::{Task, TaskCategory};
use crate::domain::repositories::{IdeationSessionRepository, TaskRepository};

/// Map a TaskCategory to its conventional commit type prefix.
///
/// | Category | Commit Type |
/// |---|---|
/// | Regular | `feat` |
/// | PlanMerge | `feat` |
pub(super) fn category_to_commit_type(category: &TaskCategory) -> &'static str {
    match category {
        TaskCategory::Regular => "feat",
        TaskCategory::PlanMerge => "feat",
    }
}

/// Derive the conventional commit type via majority-wins across task categories.
///
/// Maps each task's category to a commit type, counts votes, and returns the type
/// with the most votes. Ties are broken by variant priority:
/// feat > fix > refactor > docs > test > perf > chore.
/// Falls back to `"feat"` if the task list is empty.
pub(super) fn derive_commit_type(tasks: &[Task]) -> &'static str {
    use std::collections::HashMap;

    // Priority order for tie-breaking (lower index = higher priority)
    const PRIORITY: &[&str] = &["feat", "fix", "refactor", "docs", "test", "perf", "chore"];

    let mut votes: HashMap<&'static str, usize> = HashMap::new();
    for task in tasks {
        let commit_type = category_to_commit_type(&task.category);
        *votes.entry(commit_type).or_insert(0) += 1;
    }

    if votes.is_empty() {
        return "feat";
    }

    let max_votes = *votes.values().max().unwrap_or(&0);

    // Among types with max votes, pick the highest-priority one
    PRIORITY
        .iter()
        .find(|&&t| votes.get(t).copied().unwrap_or(0) == max_votes)
        .copied()
        .unwrap_or("feat")
}

/// Build a descriptive squash commit message for plan merge tasks.
///
/// Fetches the live session title and sibling tasks to construct:
/// `$derived_type: $session_title\n\nPlan branch: {branch}\nTasks ({n}):\n- ...`
///
/// Fallback chain for subject:
/// 1. `session.title` (live fetch) — set by ralphx-utility-session-namer or user rename
/// 2. First sibling task title — if session title is NULL
/// 3. `"Merge plan into {target_branch}"` — no session title, no tasks
///
/// Task list is capped at 20 entries with `(+N more)` overflow.
pub(super) async fn build_plan_merge_commit_msg(
    ideation_session_id: &IdeationSessionId,
    source_branch: &str,
    target_branch: &str,
    task_repo: &dyn TaskRepository,
    session_repo: &dyn IdeationSessionRepository,
) -> String {
    // Fetch sibling tasks for this ideation session
    let sibling_tasks = task_repo
        .get_by_ideation_session(ideation_session_id)
        .await
        .unwrap_or_default();

    // Fetch live session title
    let session_title = session_repo
        .get_by_id(ideation_session_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| s.title);

    // Derive commit type from sibling task categories
    let commit_type = derive_commit_type(&sibling_tasks);

    // Determine subject with fallback chain
    let subject = session_title
        .as_deref()
        .map(str::to_owned)
        .or_else(|| sibling_tasks.first().map(|t| t.title.clone()))
        .unwrap_or_else(|| format!("Merge plan into {}", target_branch));

    // Build task list body (capped at 20)
    let task_count = sibling_tasks.len();
    let mut body = format!("Plan branch: {}", source_branch);

    if task_count > 0 {
        body.push_str(&format!("\nTasks ({}):", task_count));
        let display_count = task_count.min(20);
        for task in sibling_tasks.iter().take(display_count) {
            body.push_str(&format!("\n- {}", task.title));
        }
        if task_count > 20 {
            body.push_str(&format!("\n(+{} more)", task_count - 20));
        }
    }

    format!("{}: {}\n\n{}", commit_type, subject, body)
}

/// Build a squash commit message for regular (non-plan-merge) tasks.
///
/// Format: `{task_title}\n\n{task_description}\n\nTask UUID: {task_id}`
pub(super) fn build_squash_commit_msg(
    title: &str,
    description: Option<&str>,
    task_id: &str,
) -> String {
    let subject = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let subject = if subject.is_empty() {
        "Untitled task".to_string()
    } else {
        subject
    };

    let mut message = subject;
    if let Some(description) = description.map(str::trim).filter(|value| !value.is_empty()) {
        message.push_str("\n\n");
        message.push_str(description);
    }

    message.push_str("\n\nTask UUID: ");
    message.push_str(task_id);
    message
}
