// ProjectScope Axum extractor and ProjectScopeGuard trait
//
// Reads the X-RalphX-Project-Scope header (comma-separated project IDs) and makes
// it available to handlers as ProjectScope.
//
// ProjectScope(None)  → no header present → unrestricted (backward compatible for internal agents)
// ProjectScope(Some)  → scoped to listed projects only (set by external MCP server per API key)

use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ideation::IdeationSession, project::Project, review::Review, task::Task,
};
use crate::http_server::types::HttpError;

// ============================================================================
// ProjectScope extractor
// ============================================================================

/// Project scope restriction parsed from `X-RalphX-Project-Scope` header.
///
/// Injected by the external MCP server (`:3848`) when forwarding requests on
/// behalf of an API key that is scoped to specific projects.
///
/// - `ProjectScope(None)`  → header absent → request is unrestricted (internal agent, no key)
/// - `ProjectScope(Some(ids))` → request may only access the listed projects
#[derive(Debug, Clone)]
pub struct ProjectScope(pub Option<Vec<ProjectId>>);

impl ProjectScope {
    /// Returns true when no scope restriction applies (internal request).
    pub fn is_unrestricted(&self) -> bool {
        self.0.is_none()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for ProjectScope
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let scope = parts
            .headers
            .get("x-ralphx-project-scope")
            .and_then(|v| v.to_str().ok())
            .map(parse_project_scope_header);
        Ok(ProjectScope(scope))
    }
}

/// Parse a comma-separated header value into a list of `ProjectId`s.
/// Empty strings and whitespace-only segments are skipped.
pub(crate) fn parse_project_scope_header(value: &str) -> Vec<ProjectId> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| ProjectId::from_string(s.to_string()))
        .collect()
}

// ============================================================================
// ProjectScopeGuard trait
// ============================================================================

/// Trait implemented by all project-owned entities to enforce scope boundaries.
///
/// The default implementation of `assert_project_scope` returns 403 Forbidden
/// when the entity's project is not included in the caller's allowed scope.
/// When scope is `ProjectScope(None)` (internal requests without the header),
/// access is always granted — this preserves full backward compatibility.
///
/// # Usage in handlers
///
/// ```rust,ignore
/// let task = repo.get_by_id(&task_id).await?.ok_or(StatusCode::NOT_FOUND)?;
/// task.assert_project_scope(&scope).map_err(|e| e.status)?;
/// ```
pub trait ProjectScopeGuard {
    /// Returns the project this entity belongs to.
    fn project_id(&self) -> &ProjectId;

    /// Asserts that the entity's project is within the caller's allowed scope.
    ///
    /// - `ProjectScope(None)` → unrestricted → `Ok(())`
    /// - `ProjectScope(Some(ids))` and entity's `project_id` is in `ids` → `Ok(())`
    /// - `ProjectScope(Some(ids))` and entity's `project_id` is NOT in `ids` → 403 Forbidden
    fn assert_project_scope(&self, scope: &ProjectScope) -> Result<(), HttpError> {
        if let ProjectScope(Some(allowed)) = scope {
            if !allowed.contains(self.project_id()) {
                return Err(HttpError {
                    status: StatusCode::FORBIDDEN,
                    message: Some("API key does not have access to this project".to_string()),
                });
            }
        }
        Ok(())
    }
}

// ============================================================================
// ProjectScopeGuard implementations for project-owned entities
// ============================================================================

impl ProjectScopeGuard for Task {
    fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
}

impl ProjectScopeGuard for IdeationSession {
    fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
}

/// For `Project` itself, the project IS the scope boundary: its own `id`.
impl ProjectScopeGuard for Project {
    fn project_id(&self) -> &ProjectId {
        &self.id
    }
}

impl ProjectScopeGuard for Review {
    fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[path = "project_scope_tests.rs"]
mod tests;
