// SQLite implementation of RunningAgentRegistry
//
// Persists running agent PIDs to the running_agents table so they survive app restarts.
// On restart, boot-scoped cleanup kills orphaned processes before new agents are spawned.
//
// All rusqlite calls go through DbConnection::run() (spawn_blocking + blocking_lock)
// to prevent blocking the tokio async runtime / timer driver.

use async_trait::async_trait;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::DbConnection;
use crate::AppError;
use crate::domain::services::{
    is_process_alive, kill_process, RunningAgentInfo, RunningAgentKey, RunningAgentRegistry,
};

/// SQLite-backed implementation of RunningAgentRegistry.
/// Persists agent PIDs across app restarts for orphan cleanup.
pub struct SqliteRunningAgentRegistry {
    db: DbConnection,
    /// In-memory map for cancellation tokens (not persisted to SQLite)
    tokens: Arc<Mutex<HashMap<RunningAgentKey, CancellationToken>>>,
}

impl SqliteRunningAgentRegistry {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
            tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Parse a `RunningAgentInfo` from a SELECT row with column order:
///   0: pid, 1: conversation_id, 2: agent_run_id, 3: started_at, 4: worktree_path, 5: last_active_at, 6: model
///
/// Extracts the 4× duplicated parsing logic from `unregister`, `get`, `try_register`,
/// and `cleanup_stale_entry` into one authoritative location.
fn parse_running_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunningAgentInfo> {
    let pid: u32 = row.get(0)?;
    let conversation_id: String = row.get(1)?;
    let agent_run_id: String = row.get(2)?;
    let started_at_str: String = row.get(3)?;
    let worktree_path: Option<String> = row.get(4)?;
    let last_active_at_str: Option<String> = row.get(5)?;
    let model: Option<String> = row.get(6)?;
    let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let last_active_at = last_active_at_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
    });
    Ok(RunningAgentInfo {
        pid,
        conversation_id,
        agent_run_id,
        started_at,
        worktree_path,
        cancellation_token: None, // Populated from in-memory token map by caller
        last_active_at,
        model,
    })
}

#[async_trait]
impl RunningAgentRegistry for SqliteRunningAgentRegistry {
    async fn register(
        &self,
        key: RunningAgentKey,
        pid: u32,
        conversation_id: String,
        agent_run_id: String,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
    ) {
        // Check for existing agent and stop it if still alive
        {
            let ctx_type = key.context_type.clone();
            let ctx_id = key.context_id.clone();
            let existing = self
                .db
                .run(move |conn| {
                    Ok(conn
                        .query_row(
                            "SELECT pid, worktree_path FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                            rusqlite::params![ctx_type, ctx_id],
                            |row| {
                                let old_pid: u32 = row.get(0)?;
                                let old_worktree: Option<String> = row.get(1)?;
                                Ok((old_pid, old_worktree))
                            },
                        )
                        .ok())
                })
                .await
                .unwrap_or(None);

            if let Some((old_pid, _old_worktree)) = existing {
                if old_pid != pid && is_process_alive(old_pid) {
                    tracing::warn!(
                        old_pid,
                        new_pid = pid,
                        context_type = %key.context_type,
                        context_id = %key.context_id,
                        "Detected orphaned agent process — stopping before re-registration"
                    );
                    // Cancel old cancellation token
                    {
                        let mut tokens = self.tokens.lock().await;
                        if let Some(old_token) = tokens.remove(&key) {
                            old_token.cancel();
                        }
                    }
                    kill_process(old_pid);
                }
            }
        }

        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();
        let wt_path = worktree_path;
        let _ = self
            .db
            .run(move |conn| {
                let started_at = chrono::Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT OR REPLACE INTO running_agents (context_type, context_id, pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        ctx_type,
                        ctx_id,
                        pid,
                        conversation_id,
                        agent_run_id,
                        started_at,
                        wt_path,
                        Option::<String>::None
                    ],
                )?;
                Ok(())
            })
            .await;

        // Store cancellation token in memory (not persisted to SQLite)
        if let Some(token) = cancellation_token {
            let mut tokens = self.tokens.lock().await;
            tokens.insert(key, token);
        }
    }

    async fn unregister(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
    ) -> Option<RunningAgentInfo> {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();
        let run_id = agent_run_id.to_string();

        // Read + delete atomically under the same lock in spawn_blocking
        let info = self
            .db
            .run(move |conn| {
                // Read the row only if agent_run_id matches (ownership check prevents a finishing
                // agent from accidentally deleting a newer agent's slot for the same context).
                let info = match conn.query_row(
                    "SELECT pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model FROM running_agents WHERE context_type = ?1 AND context_id = ?2 AND agent_run_id = ?3",
                    rusqlite::params![&ctx_type, &ctx_id, &run_id],
                    parse_running_agent_row,
                ) {
                    Ok(info) => info,
                    Err(_) => return Ok(None),
                };

                // Delete the row (scoped to matching agent_run_id)
                match conn.execute(
                    "DELETE FROM running_agents WHERE context_type = ?1 AND context_id = ?2 AND agent_run_id = ?3",
                    rusqlite::params![&ctx_type, &ctx_id, &run_id],
                ) {
                    Ok(0) => {
                        tracing::debug!(
                            context_type = %ctx_type,
                            context_id = %ctx_id,
                            "unregister: 0 rows deleted — entry already gone"
                        );
                    }
                    Ok(_) => {} // deleted successfully
                    Err(e) => {
                        tracing::error!(
                            context_type = %ctx_type,
                            context_id = %ctx_id,
                            error = %e,
                            "unregister: DELETE failed"
                        );
                    }
                }

                Ok(Some(info))
            })
            .await
            .unwrap_or(None);

        // Attach cancellation token from in-memory map
        let token = {
            let mut tokens = self.tokens.lock().await;
            tokens.remove(key)
        };

        info.map(|mut i| {
            i.cancellation_token = token;
            i
        })
    }

    async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();

        let info = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                        rusqlite::params![ctx_type, ctx_id],
                        parse_running_agent_row,
                    )
                    .ok())
            })
            .await
            .unwrap_or(None);

        // Attach cancellation token from in-memory map
        let tokens = self.tokens.lock().await;
        info.map(|mut i| {
            i.cancellation_token = tokens.get(key).cloned();
            i
        })
    }

    async fn is_running(&self, key: &RunningAgentKey) -> bool {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();

        self.db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                        rusqlite::params![ctx_type, ctx_id],
                        |_| Ok(()),
                    )
                    .is_ok())
            })
            .await
            .unwrap_or(false)
    }

    async fn stop(&self, key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();

        let agent_run_id = self
            .db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT agent_run_id FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                        rusqlite::params![ctx_type, ctx_id],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap_or_default())
            })
            .await
            .unwrap_or_default();

        let info = self.unregister(key, &agent_run_id).await;

        if let Some(ref agent_info) = info {
            // Cancel the async task before killing the process
            if let Some(ref token) = agent_info.cancellation_token {
                token.cancel();
            }
            // Note: worktree process cleanup (lsof scan) is intentionally NOT done here.
            // kill_worktree_processes() is a synchronous blocking call that can hang the Tokio
            // thread indefinitely. pre_merge_cleanup step 0b handles the worktree scan via
            // kill_worktree_processes_async (with timeout + kill_on_drop). stop() only needs
            // to send SIGTERM — sufficient for cooperative cancellation.
            kill_process(agent_info.pid);
        }

        Ok(info)
    }

    async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
        let mut results = self
            .db
            .run(move |conn| {
                let mut stmt = match conn.prepare(
                    "SELECT context_type, context_id, pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model FROM running_agents",
                ) {
                    Ok(stmt) => stmt,
                    Err(_) => return Ok(Vec::new()),
                };

                let mut results = Vec::new();
                let mut rows = match stmt.query([]) {
                    Ok(rows) => rows,
                    Err(_) => return Ok(Vec::new()),
                };

                while let Ok(Some(row)) = rows.next() {
                    let context_type: String = match row.get(0) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let context_id: String = match row.get(1) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let pid: u32 = match row.get(2) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let conversation_id: String = match row.get(3) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let agent_run_id: String = match row.get(4) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let started_at_str: String = match row.get(5) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let worktree_path: Option<String> = match row.get(6) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let last_active_at_str: Option<String> = row.get(7).unwrap_or_default();
                    let model: Option<String> = row.get(8).unwrap_or_default();
                    let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now());
                    let last_active_at = last_active_at_str.and_then(|s| {
                        chrono::DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .ok()
                    });

                    results.push((
                        RunningAgentKey {
                            context_type,
                            context_id,
                        },
                        RunningAgentInfo {
                            pid,
                            conversation_id,
                            agent_run_id,
                            started_at,
                            worktree_path,
                            cancellation_token: None,
                            last_active_at,
                            model,
                        },
                    ));
                }

                Ok(results)
            })
            .await
            .unwrap_or_default();

        // Attach cancellation tokens from in-memory map
        let tokens = self.tokens.lock().await;
        for (key, info) in &mut results {
            info.cancellation_token = tokens.get(key).cloned();
        }

        results
    }

    async fn list_by_context_type(
        &self,
        context_type: &str,
    ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
        let ctx_type = context_type.to_string();

        let mut results = self
            .db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT context_type, context_id, pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model FROM running_agents WHERE context_type = ?1",
                )?;

                let mut results = Vec::new();
                let mut rows = stmt.query(rusqlite::params![ctx_type])?;

                while let Some(row) = rows.next()? {
                    let context_type: String = row.get(0)?;
                    let context_id: String = row.get(1)?;
                    let pid: u32 = row.get(2)?;
                    let conversation_id: String = row.get(3)?;
                    let agent_run_id: String = row.get(4)?;
                    let started_at_str: String = row.get(5)?;
                    let worktree_path: Option<String> = row.get(6)?;
                    let last_active_at_str: Option<String> = row.get(7)?;
                    let model: Option<String> = row.get(8)?;

                    let started_at = chrono::DateTime::parse_from_rfc3339(&started_at_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .map_err(|e| AppError::Database(format!(
                            "Failed to parse started_at '{}': {}", started_at_str, e
                        )))?;
                    let last_active_at = last_active_at_str
                        .map(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .map_err(|e| AppError::Database(format!(
                                    "Failed to parse last_active_at '{}': {}", s, e
                                )))
                        })
                        .transpose()?;

                    results.push((
                        RunningAgentKey { context_type, context_id },
                        RunningAgentInfo {
                            pid,
                            conversation_id,
                            agent_run_id,
                            started_at,
                            worktree_path,
                            cancellation_token: None,
                            last_active_at,
                            model,
                        },
                    ));
                }

                Ok(results)
            })
            .await
            .map_err(|e| e.to_string())?;

        // Attach cancellation tokens from in-memory map
        let tokens = self.tokens.lock().await;
        for (key, info) in &mut results {
            info.cancellation_token = tokens.get(key).cloned();
        }

        Ok(results)
    }

    async fn stop_all(&self) -> Vec<RunningAgentKey> {
        // Cancel all tokens first
        {
            let mut tokens = self.tokens.lock().await;
            for token in tokens.values() {
                token.cancel();
            }
            tokens.clear();
        }

        // Read all entries, kill processes, then clear table
        let entries = self.list_all().await;

        let mut stopped = Vec::new();
        for (key, info) in &entries {
            kill_process(info.pid);
            stopped.push(key.clone());
        }

        // Clear table
        let _ = self
            .db
            .run(move |conn| {
                conn.execute("DELETE FROM running_agents", [])?;
                Ok(())
            })
            .await;

        stopped
    }

    async fn stop_all_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Vec<RunningAgentKey> {
        let entries: Vec<(RunningAgentKey, RunningAgentInfo)> = self
            .list_all()
            .await
            .into_iter()
            .filter(|(_, info)| info.started_at < cutoff)
            .collect();
        if entries.is_empty() {
            return Vec::new();
        }

        for (_, info) in &entries {
            kill_process(info.pid);
        }

        let cutoff_str = cutoff.to_rfc3339();
        let delete_result = self
            .db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM running_agents WHERE started_at < ?1",
                    [&cutoff_str],
                )?;
                Ok(())
            })
            .await;

        if let Err(e) = delete_result {
            tracing::warn!(
                error = %e,
                "stop_all_started_before: failed to delete previous-session registry rows"
            );
        }

        let mut stopped = Vec::new();
        for (key, _) in entries {
            if self.get(&key).await.is_none() {
                let token = {
                    let mut tokens = self.tokens.lock().await;
                    tokens.remove(&key)
                };
                if let Some(token) = token {
                    token.cancel();
                }
                stopped.push(key);
            }
        }

        stopped
    }

    async fn update_heartbeat(&self, key: &RunningAgentKey, at: chrono::DateTime<chrono::Utc>) {
        let at_str = at.to_rfc3339();
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();

        let _ = self
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE running_agents SET last_active_at = ?1 WHERE context_type = ?2 AND context_id = ?3",
                    rusqlite::params![at_str, ctx_type, ctx_id],
                )?;
                Ok(())
            })
            .await;
    }

    async fn try_register(
        &self,
        key: RunningAgentKey,
        conversation_id: String,
        agent_run_id: String,
    ) -> Result<(), RunningAgentInfo> {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();
        let conv_id = conversation_id;
        let run_id = agent_run_id;

        // Hold conn lock across check+insert for atomicity (inside spawn_blocking)
        let result = self
            .db
            .run(move |conn| {
                // Check for existing registration
                let existing = conn
                    .query_row(
                        "SELECT pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                        rusqlite::params![&ctx_type, &ctx_id],
                        parse_running_agent_row,
                    )
                    .ok();

                if let Some(info) = existing {
                    return Ok(Err(info));
                }

                // Insert placeholder registration (pid=0, no worktree)
                let started_at = chrono::Utc::now().to_rfc3339();
                if let Err(e) = conn.execute(
                    "INSERT INTO running_agents (context_type, context_id, pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at) VALUES (?1, ?2, 0, ?3, ?4, ?5, NULL, NULL)",
                    rusqlite::params![&ctx_type, &ctx_id, &conv_id, &run_id, &started_at],
                ) {
                    tracing::error!(
                        context_type = %ctx_type,
                        context_id = %ctx_id,
                        error = %e,
                        "try_register: INSERT failed — agent slot may not be reserved"
                    );
                }

                Ok(Ok(()))
            })
            .await;

        // Handle spawn_blocking join error — degrade gracefully
        let inner_result = match result {
            Ok(inner) => inner,
            Err(app_err) => {
                tracing::error!("try_register: spawn_blocking failed: {app_err}");
                Ok(())
            }
        };

        // Attach cancellation token if existing agent was found
        match inner_result {
            Err(mut info) => {
                let tokens = self.tokens.lock().await;
                info.cancellation_token = tokens.get(&key).cloned();
                Err(info)
            }
            Ok(()) => Ok(()),
        }
    }

    async fn update_agent_process(
        &self,
        key: &RunningAgentKey,
        pid: u32,
        conversation_id: &str,
        agent_run_id: &str,
        worktree_path: Option<String>,
        cancellation_token: Option<CancellationToken>,
        model: Option<String>,
    ) -> Result<(), String> {
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();
        let conv_id = conversation_id.to_string();
        let run_id = agent_run_id.to_string();
        let wt_path = worktree_path;

        let db_result = self
            .db
            .run(move |conn| {
                Ok(match conn.execute(
                    "UPDATE running_agents SET pid = ?1, worktree_path = ?2, agent_run_id = ?3, model = ?4 WHERE context_type = ?5 AND context_id = ?6",
                    rusqlite::params![pid, &wt_path, &run_id, &model, &ctx_type, &ctx_id],
                ) {
                    Ok(0) => {
                        // TOCTOU recovery: the placeholder row was pruned between try_register
                        // and this call. Re-insert the full registration so the agent is tracked.
                        tracing::warn!(
                            context_type = %ctx_type,
                            context_id = %ctx_id,
                            pid,
                            "update_agent_process: 0 rows affected — re-inserting full registration"
                        );
                        let started_at = chrono::Utc::now().to_rfc3339();
                        conn.execute(
                            "INSERT OR REPLACE INTO running_agents (context_type, context_id, pid, conversation_id, agent_run_id, started_at, worktree_path, last_active_at, model) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)",
                            rusqlite::params![&ctx_type, &ctx_id, pid, &conv_id, &run_id, &started_at, &wt_path, &model],
                        ).map(|_| ()).map_err(|e| {
                            tracing::error!(
                                context_type = %ctx_type,
                                context_id = %ctx_id,
                                error = %e,
                                "update_agent_process: INSERT OR REPLACE failed after pruned row"
                            );
                            e.to_string()
                        })
                    }
                    Ok(_) => Ok(()),
                    Err(e) => {
                        tracing::error!(
                            context_type = %ctx_type,
                            context_id = %ctx_id,
                            error = %e,
                            "update_agent_process: UPDATE failed"
                        );
                        Err(e.to_string())
                    }
                })
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));

        // Always store the cancellation token regardless of DB result —
        // the in-memory token is needed to cancel the process even if DB is inconsistent.
        if let Some(token) = cancellation_token {
            let mut tokens = self.tokens.lock().await;
            tokens.insert(key.clone(), token);
        }

        db_result
    }

    async fn cleanup_stale_entry(
        &self,
        key: &RunningAgentKey,
    ) -> Result<Option<RunningAgentInfo>, String> {
        // Read the current entry (if any)
        let info = self.get(key).await;
        let info = match info {
            Some(i) => i,
            None => return Ok(None),
        };

        // Only remove if the process is actually dead (Proof Obligation 7):
        // never deletes a live agent's row.
        if is_process_alive(info.pid) {
            tracing::debug!(
                pid = info.pid,
                context_type = %key.context_type,
                context_id = %key.context_id,
                "cleanup_stale_entry: process is still alive, skipping"
            );
            return Ok(None);
        }

        // Process is dead — delete the row unconditionally (no agent_run_id guard,
        // because stale cleanup is intentionally scoped to dead processes only).
        let ctx_type = key.context_type.clone();
        let ctx_id = key.context_id.clone();
        let ctx_type_log = key.context_type.clone();
        let ctx_id_log = key.context_id.clone();
        let delete_result = self
            .db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
                    rusqlite::params![ctx_type, ctx_id],
                )?;
                Ok(())
            })
            .await;

        if let Err(e) = delete_result {
            tracing::error!(
                context_type = %ctx_type_log,
                context_id = %ctx_id_log,
                error = %e,
                "cleanup_stale_entry: DELETE failed"
            );
            return Err(e.to_string());
        }

        // Remove the in-memory cancellation token (already cancelled since process is dead)
        let token = {
            let mut tokens = self.tokens.lock().await;
            tokens.remove(key)
        };

        tracing::info!(
            context_type = %key.context_type,
            context_id = %key.context_id,
            pid = info.pid,
            "cleanup_stale_entry: removed stale entry for dead process"
        );

        Ok(Some(RunningAgentInfo {
            cancellation_token: token,
            ..info
        }))
    }
}

#[cfg(test)]
#[path = "sqlite_running_agent_registry_tests.rs"]
mod tests;
