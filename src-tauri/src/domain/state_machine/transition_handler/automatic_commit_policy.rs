use std::path::Path;

use crate::domain::entities::Project;

/// Returns true when PR mode must protect the project's primary checkout from an
/// automatic commit. Path-resolution failures fail closed.
pub(super) fn protects_primary_checkout(project: &Project, candidate: &Path) -> bool {
    if !project.github_pr_enabled {
        return false;
    }

    let project_root = Path::new(&project.working_directory);
    match (candidate.canonicalize(), project_root.canonicalize()) {
        (Ok(candidate), Ok(project_root)) => candidate == project_root,
        _ => true,
    }
}

#[cfg(test)]
#[path = "automatic_commit_policy_tests.rs"]
mod tests;
