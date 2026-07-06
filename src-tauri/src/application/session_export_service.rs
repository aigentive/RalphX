// Session export/import service
// Bypasses per-repo abstractions to perform raw SQL across multiple tables in a single transaction.
// Export: db.run() (read-only) | Import: db.run_transaction() (atomic writes)

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

// ============================================================================
// DTOs
// ============================================================================

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExport {
    pub schema_version: u32,
    pub exported_at: String,
    pub source_instance: SourceInstance,
    pub session: SessionData,
    pub plan_versions: Vec<PlanVersionData>,
    pub proposals: Vec<ProposalData>,
    pub dependencies: Vec<DependencyData>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInstance {
    pub project_name: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionData {
    pub title: Option<String>,
    pub status: String,
    pub team_mode: String,
    pub verification: Option<VerificationExportData>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationExportData {
    pub status: String,
    pub in_progress: bool,
    pub generation: i32,
    pub current_round: Option<u32>,
    pub max_rounds: Option<u32>,
    pub gap_count: u32,
    pub gap_score: Option<u32>,
    pub convergence_reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanVersionData {
    pub version: u32,
    pub name: String,
    pub content: Option<String>,
    pub content_type: String,
    pub created_at: String,
    pub created_by: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalData {
    pub index: usize,
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub steps: Option<Vec<String>>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub suggested_priority: String,
    pub priority_score: i32,
    pub priority_reason: Option<String>,
    pub priority_factors: Option<PriorityFactorsData>,
    pub estimated_complexity: String,
    pub user_priority: Option<String>,
    pub user_modified: bool,
    pub sort_order: i32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityFactorsData {
    pub dependency: i32,
    pub business_value: i32,
    pub technical_risk: i32,
    pub user_demand: i32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyData {
    pub from_index: usize,
    pub to_index: usize,
    pub source: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSession {
    pub session_id: String,
    pub title: Option<String>,
    pub proposal_count: usize,
    pub plan_version_count: usize,
}

// ============================================================================
// Service
// ============================================================================

pub struct SessionExportService {
    db: DbConnection,
}

impl SessionExportService {
    pub fn new(db: DbConnection) -> Self {
        Self { db }
    }

    /// Export a complete ideation session as a portable SessionExport struct.
    /// Returns AppError::NotFound if session doesn't exist or belongs to a different project.
    pub async fn export(&self, session_id: &str, project_id: &str) -> AppResult<SessionExport> {
        let session_id = session_id.to_string();
        let project_id = project_id.to_string();

        self.db
            .run(move |conn| {
                // Load session with project-level auth check
                // We query each field separately for clear type inference
                struct SessionRow {
                    title: Option<String>,
                    status: String,
                    team_mode: String,
                    verification_status: String,
                    verification_in_progress: bool,
                    verification_generation: i32,
                    verification_current_round: Option<u32>,
                    verification_max_rounds: Option<u32>,
                    verification_gap_count: u32,
                    verification_gap_score: Option<u32>,
                    verification_convergence_reason: Option<String>,
                    plan_artifact_id: Option<String>,
                    inherited_plan_artifact_id: Option<String>,
                    project_name: Option<String>,
                }

                let maybe_session: Option<SessionRow> = conn
                    .query_row(
                        "SELECT s.title, s.status, COALESCE(s.team_mode, 'solo'), s.verification_status, \
                         s.verification_in_progress, s.verification_generation, s.verification_current_round, \
                         s.verification_max_rounds, s.verification_gap_count, s.verification_gap_score, \
                         s.verification_convergence_reason, s.plan_artifact_id, \
                         s.inherited_plan_artifact_id, p.name \
                         FROM ideation_sessions s \
                         LEFT JOIN projects p ON p.id = s.project_id \
                         WHERE s.id = ?1 AND s.project_id = ?2",
                        rusqlite::params![session_id, project_id],
                        |row| {
                            Ok(SessionRow {
                                title: row.get(0)?,
                                status: row.get(1)?,
                                team_mode: row.get(2)?,
                                verification_status: row.get(3)?,
                                verification_in_progress: row.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
                                verification_generation: row.get::<_, Option<i32>>(5)?.unwrap_or(0),
                                verification_current_round: row.get(6)?,
                                verification_max_rounds: row.get(7)?,
                                verification_gap_count: row.get::<_, Option<u32>>(8)?.unwrap_or(0),
                                verification_gap_score: row.get(9)?,
                                verification_convergence_reason: row.get(10)?,
                                plan_artifact_id: row.get(11)?,
                                inherited_plan_artifact_id: row.get(12)?,
                                project_name: row.get(13)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(AppError::from)?;

                let session_row = maybe_session.ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Session {} not found or does not belong to project {}",
                        session_id, project_id
                    ))
                })?;

                let session_data = SessionData {
                    title: session_row.title,
                    status: session_row.status,
                    team_mode: session_row.team_mode,
                    verification: Some(VerificationExportData {
                        status: session_row.verification_status,
                        in_progress: session_row.verification_in_progress,
                        generation: session_row.verification_generation,
                        current_round: session_row.verification_current_round,
                        max_rounds: session_row.verification_max_rounds,
                        gap_count: session_row.verification_gap_count,
                        gap_score: session_row.verification_gap_score,
                        convergence_reason: session_row.verification_convergence_reason,
                    }),
                };

                // Resolve version chain if plan exists — own plan_artifact_id takes precedence,
                // inherited_plan_artifact_id used when own is NULL (followup sessions)
                let plan_versions = if let Some(ref root_id) = session_row.plan_artifact_id {
                    Self::walk_version_chain(conn, root_id, &session_id)?
                } else if let Some(ref inherited_id) = session_row.inherited_plan_artifact_id {
                    Self::walk_version_chain(conn, inherited_id, &session_id)?
                } else {
                    vec![]
                };

                // Load proposals
                let mut proposal_stmt = conn.prepare(
                    "SELECT id, title, description, category, steps, acceptance_criteria, \
                     suggested_priority, priority_score, priority_reason, priority_factors, \
                     estimated_complexity, user_priority, user_modified, sort_order \
                     FROM task_proposals \
                     WHERE session_id = ?1 \
                     ORDER BY sort_order",
                )?;

                let mut proposals: Vec<ProposalData> = Vec::new();
                let mut proposal_id_to_index: HashMap<String, usize> = HashMap::new();

                struct ProposalRow {
                    id: String,
                    title: String,
                    description: Option<String>,
                    category: String,
                    steps: Option<String>,
                    acceptance_criteria: Option<String>,
                    suggested_priority: String,
                    priority_score: i32,
                    priority_reason: Option<String>,
                    priority_factors: Option<String>,
                    estimated_complexity: String,
                    user_priority: Option<String>,
                    user_modified: i32,
                    sort_order: i32,
                }

                let raw_proposals: Vec<ProposalRow> = proposal_stmt
                    .query_map([&session_id], |row| {
                        Ok(ProposalRow {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            description: row.get(2)?,
                            category: row.get(3)?,
                            steps: row.get(4)?,
                            acceptance_criteria: row.get(5)?,
                            suggested_priority: row.get(6)?,
                            priority_score: row.get(7)?,
                            priority_reason: row.get(8)?,
                            priority_factors: row.get(9)?,
                            estimated_complexity: row.get(10)?,
                            user_priority: row.get(11)?,
                            user_modified: row.get(12)?,
                            sort_order: row.get(13)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                for (idx, row) in raw_proposals.into_iter().enumerate() {
                    proposal_id_to_index.insert(row.id, idx);

                    // Parse JSON strings to arrays
                    let steps: Option<Vec<String>> = row
                        .steps
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());
                    let acceptance_criteria: Option<Vec<String>> = row
                        .acceptance_criteria
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());
                    let priority_factors: Option<PriorityFactorsData> = row
                        .priority_factors
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());

                    proposals.push(ProposalData {
                        index: idx,
                        title: row.title,
                        description: row.description,
                        category: row.category,
                        steps,
                        acceptance_criteria,
                        suggested_priority: row.suggested_priority,
                        priority_score: row.priority_score,
                        priority_reason: row.priority_reason,
                        priority_factors,
                        estimated_complexity: row.estimated_complexity,
                        user_priority: row.user_priority,
                        user_modified: row.user_modified != 0,
                        sort_order: row.sort_order,
                    });
                }

                // Load dependencies and convert to index-based
                let proposal_ids_in: Vec<String> = proposal_id_to_index.keys().cloned().collect();
                let mut dependencies: Vec<DependencyData> = Vec::new();

                if !proposal_ids_in.is_empty() {
                    // Build placeholders for IN clause using positional params
                    let placeholders: String = (1..=proposal_ids_in.len())
                        .map(|i| format!("?{}", i))
                        .collect::<Vec<_>>()
                        .join(", ");

                    let dep_query = format!(
                        "SELECT proposal_id, depends_on_proposal_id, source \
                         FROM proposal_dependencies \
                         WHERE proposal_id IN ({})",
                        placeholders
                    );

                    let mut dep_stmt = conn.prepare(&dep_query)?;
                    let params: Vec<Box<dyn rusqlite::ToSql>> = proposal_ids_in
                        .into_iter()
                        .map(|s| -> Box<dyn rusqlite::ToSql> { Box::new(s) })
                        .collect();
                    let params_refs: Vec<&dyn rusqlite::ToSql> =
                        params.iter().map(|p| p.as_ref()).collect();

                    struct DepRow {
                        proposal_id: String,
                        depends_on_id: String,
                        source: String,
                    }

                    let dep_rows: Vec<DepRow> = dep_stmt
                        .query_map(params_refs.as_slice(), |row| {
                            Ok(DepRow {
                                proposal_id: row.get(0)?,
                                depends_on_id: row.get(1)?,
                                source: row.get(2)?,
                            })
                        })?
                        .collect::<Result<Vec<_>, _>>()?;

                    for dep in dep_rows {
                        if let (Some(&from_idx), Some(&to_idx)) = (
                            proposal_id_to_index.get(&dep.proposal_id),
                            proposal_id_to_index.get(&dep.depends_on_id),
                        ) {
                            dependencies.push(DependencyData {
                                from_index: from_idx,
                                to_index: to_idx,
                                source: dep.source,
                            });
                        }
                    }
                }

                info!(
                    "Exported session {} with {} proposals and {} plan versions",
                    session_id,
                    proposals.len(),
                    plan_versions.len()
                );

                Ok(SessionExport {
                    schema_version: 1,
                    exported_at: Utc::now().to_rfc3339(),
                    source_instance: SourceInstance {
                        project_name: session_row.project_name.unwrap_or_default(),
                    },
                    session: session_data,
                    plan_versions,
                    proposals,
                    dependencies,
                })
            })
            .await
    }

    /// Walk the version chain forward from the root artifact ID.
    /// Returns versions in chronological order (root first).
    /// Guards: HashSet to prevent infinite loops, cap at 1000 versions.
    fn walk_version_chain(
        conn: &rusqlite::Connection,
        root_id: &str,
        session_id: &str,
    ) -> AppResult<Vec<PlanVersionData>> {
        const MAX_VERSIONS: usize = 1000;
        let mut versions = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut current_id = root_id.to_string();

        loop {
            if visited.contains(&current_id) {
                warn!(
                    "Version chain cycle detected at {} for session {}",
                    current_id, session_id
                );
                break;
            }
            visited.insert(current_id.clone());

            struct ArtifactRow {
                version: u32,
                name: String,
                content: Option<String>,
                content_type: String,
                created_at: String,
                created_by: String,
            }

            let maybe_artifact: Option<ArtifactRow> = conn
                .query_row(
                    "SELECT version, name, content_text, content_type, created_at, created_by \
                     FROM artifacts WHERE id = ?1",
                    [&current_id],
                    |row| {
                        Ok(ArtifactRow {
                            version: row.get(0)?,
                            name: row.get(1)?,
                            content: row.get(2)?,
                            content_type: row.get(3)?,
                            created_at: row.get(4)?,
                            created_by: row.get(5)?,
                        })
                    },
                )
                .optional()
                .map_err(AppError::from)?;

            let artifact = match maybe_artifact {
                Some(r) => r,
                None => break, // artifact disappeared, stop walking
            };

            versions.push(PlanVersionData {
                version: artifact.version,
                name: artifact.name,
                content: artifact.content,
                content_type: artifact.content_type,
                created_at: artifact.created_at,
                created_by: artifact.created_by,
            });

            if versions.len() >= MAX_VERSIONS {
                warn!(
                    "Version chain truncated at 1000 for session {}",
                    session_id
                );
                break;
            }

            // Walk forward: find next artifact that points to current as its previous
            let next_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM artifacts WHERE previous_version_id = ?1 LIMIT 1",
                    [&current_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from)?;

            match next_id {
                Some(id) => current_id = id,
                None => break,
            }
        }

        Ok(versions)
    }

    /// Import a session from JSON content into the given project.
    /// Returns ImportedSession with the new session_id and counts.
    pub async fn import(
        &self,
        json_content: &str,
        project_id: &str,
    ) -> AppResult<ImportedSession> {
        // File size guard (10MB)
        if json_content.len() > 10_485_760 {
            warn!(
                "Import rejected: content size {} bytes exceeds 10MB limit",
                json_content.len()
            );
            return Err(AppError::ImportInvalidFormat {
                detail: "File exceeds 10MB limit".into(),
            });
        }

        // Parse JSON
        let export: SessionExport = serde_json::from_str(json_content).map_err(|e| {
            AppError::ImportInvalidFormat {
                detail: format!("Invalid JSON: {}", e),
            }
        })?;

        // Validate schema version
        if export.schema_version != 1 {
            return Err(AppError::ImportVersionUnsupported {
                version: export.schema_version,
            });
        }

        // Validate dependency indices
        let proposal_count = export.proposals.len();
        for dep in &export.dependencies {
            if dep.from_index >= proposal_count || dep.to_index >= proposal_count {
                return Err(AppError::ImportInvalidDependency {
                    detail: format!(
                        "Dependency index out of bounds: from_index={}, to_index={}, proposals={}",
                        dep.from_index, dep.to_index, proposal_count
                    ),
                });
            }
        }

        // Cycle detection via Kahn's algorithm
        Self::detect_cycles(&export.dependencies, proposal_count)?;

        // Proposal count limit (500)
        if proposal_count > 500 {
            warn!(
                "Import rejected: {} proposals exceeds 500 limit",
                proposal_count
            );
            return Err(AppError::ImportInvalidFormat {
                detail: format!(
                    "Import contains {} proposals, exceeding the 500 proposal limit",
                    proposal_count
                ),
            });
        }

        let project_id = project_id.to_string();

        self.db
            .run_transaction(move |conn| {
                // Validate project exists
                let project_exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM projects WHERE id = ?1",
                        [&project_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .map_err(AppError::from)?;

                if !project_exists {
                    return Err(AppError::NotFound(format!(
                        "Project {} not found",
                        project_id
                    )));
                }

                let now = Utc::now().to_rfc3339();
                let new_session_id = Uuid::new_v4().to_string();

                // Determine title_source
                let title_source = if export.session.title.is_some() {
                    "user"
                } else {
                    "imported"
                };

                // Validate team_mode
                let team_mode = match export.session.team_mode.as_str() {
                    "solo" | "research" | "debate" => export.session.team_mode.clone(),
                    _ => {
                        warn!(
                            "Unknown team_mode '{}' during import, defaulting to 'solo'",
                            export.session.team_mode
                        );
                        "solo".to_string()
                    }
                };

                // Insert session (15-column pattern from sqlite_ideation_session_repo.rs:213)
                conn.execute(
                    "INSERT INTO ideation_sessions \
                     (id, project_id, title, title_source, status, plan_artifact_id, \
                      inherited_plan_artifact_id, seed_task_id, parent_session_id, \
                      created_at, updated_at, archived_at, converted_at, team_mode, team_config_json) \
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, ?6, ?7, NULL, NULL, ?8, NULL)",
                    rusqlite::params![
                        new_session_id,
                        project_id,
                        export.session.title,
                        title_source,
                        "active",
                        now,
                        now,
                        team_mode,
                    ],
                )
                .map_err(AppError::from)?;

                // Insert artifact chain
                let mut first_artifact_id: Option<String> = None;
                let mut latest_artifact_id: Option<String> = None;
                let mut prev_artifact_id: Option<String> = None;

                for version in &export.plan_versions {
                    let new_id = Uuid::new_v4().to_string();

                    conn.execute(
                        "INSERT INTO artifacts \
                         (id, type, name, content_type, content_text, content_path, \
                          bucket_id, task_id, process_id, created_by, version, \
                          previous_version_id, created_at, metadata_json) \
                         VALUES (?1, 'specification', ?2, ?3, ?4, NULL, \
                                 NULL, NULL, NULL, ?5, ?6, ?7, ?8, NULL)",
                        rusqlite::params![
                            new_id,
                            version.name,
                            version.content_type,
                            version.content,
                            version.created_by,
                            version.version,
                            prev_artifact_id,
                            now,
                        ],
                    )
                    .map_err(AppError::from)?;

                    if first_artifact_id.is_none() {
                        first_artifact_id = Some(new_id.clone());
                    }
                    latest_artifact_id = Some(new_id.clone());
                    prev_artifact_id = Some(new_id);
                }

                // Update session with root plan_artifact_id (first in chain)
                if let Some(ref first_id) = first_artifact_id {
                    conn.execute(
                        "UPDATE ideation_sessions SET plan_artifact_id = ?2 WHERE id = ?1",
                        rusqlite::params![new_session_id, first_id],
                    )
                    .map_err(AppError::from)?;
                }

                let plan_version_count = export.plan_versions.len();

                // Insert proposals and track index → new_id mapping
                let mut index_to_new_id: Vec<String> = Vec::with_capacity(proposal_count);

                for proposal in &export.proposals {
                    let new_proposal_id = Uuid::new_v4().to_string();
                    index_to_new_id.push(new_proposal_id.clone());

                    // Serialize steps/acceptance_criteria back to JSON strings
                    let steps_json = proposal
                        .steps
                        .as_ref()
                        .and_then(|v| serde_json::to_string(v).ok());
                    let criteria_json = proposal
                        .acceptance_criteria
                        .as_ref()
                        .and_then(|v| serde_json::to_string(v).ok());
                    let priority_factors_json = proposal
                        .priority_factors
                        .as_ref()
                        .and_then(|f| serde_json::to_string(f).ok());

                    conn.execute(
                        "INSERT INTO task_proposals \
                         (id, session_id, title, description, category, steps, acceptance_criteria, \
                          suggested_priority, priority_score, priority_reason, priority_factors, \
                          estimated_complexity, user_priority, user_modified, status, selected, \
                          created_task_id, plan_artifact_id, plan_version_at_creation, sort_order, \
                          created_at, updated_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                                 'pending', 0, NULL, ?15, ?16, ?17, ?18, ?19)",
                        rusqlite::params![
                            new_proposal_id,
                            new_session_id,
                            proposal.title,
                            proposal.description,
                            proposal.category,
                            steps_json,
                            criteria_json,
                            proposal.suggested_priority,
                            proposal.priority_score,
                            proposal.priority_reason,
                            priority_factors_json,
                            proposal.estimated_complexity,
                            proposal.user_priority,
                            proposal.user_modified as i32,
                            latest_artifact_id,
                            plan_version_count as i64,
                            proposal.sort_order,
                            now,
                            now,
                        ],
                    )
                    .map_err(AppError::from)?;
                }

                // Insert dependencies using remapped IDs
                for dep in &export.dependencies {
                    let dep_id = Uuid::new_v4().to_string();
                    let from_new_id = &index_to_new_id[dep.from_index];
                    let to_new_id = &index_to_new_id[dep.to_index];

                    conn.execute(
                        "INSERT OR IGNORE INTO proposal_dependencies \
                         (id, proposal_id, depends_on_proposal_id, source) \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![dep_id, from_new_id, to_new_id, dep.source],
                    )
                    .map_err(AppError::from)?;
                }

                info!(
                    "Imported session {} ({}) with {} proposals and {} plan versions",
                    new_session_id,
                    export.session.title.as_deref().unwrap_or("untitled"),
                    proposal_count,
                    plan_version_count
                );

                Ok(ImportedSession {
                    session_id: new_session_id,
                    title: export.session.title,
                    proposal_count,
                    plan_version_count,
                })
            })
            .await
    }

    /// Kahn's algorithm cycle detection on dependency graph.
    /// Returns Ok(()) if no cycle, Err(ImportInvalidDependency) if cycle detected.
    fn detect_cycles(dependencies: &[DependencyData], node_count: usize) -> AppResult<()> {
        if node_count == 0 || dependencies.is_empty() {
            return Ok(());
        }

        let mut in_degree = vec![0usize; node_count];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); node_count];

        for dep in dependencies {
            adj[dep.from_index].push(dep.to_index);
            in_degree[dep.to_index] += 1;
        }

        let mut queue: VecDeque<usize> = VecDeque::new();
        for (i, &deg) in in_degree.iter().enumerate() {
            if deg == 0 {
                queue.push_back(i);
            }
        }

        let mut processed = 0;
        while let Some(node) = queue.pop_front() {
            processed += 1;
            for &neighbor in &adj[node] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    queue.push_back(neighbor);
                }
            }
        }

        if processed < node_count {
            warn!("Cycle detected during import dependency validation");
            return Err(AppError::ImportInvalidDependency {
                detail: "Dependency graph contains a cycle".into(),
            });
        }

        Ok(())
    }
}
