// Migration v20260730025727: chat message blocks thinking kind

#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use nix::sys::statvfs::statvfs;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{AppError, AppResult};

/// The rebuild needs room for a full copy of the table plus the WAL that holds
/// it. Whole-database size already over-estimates the table, so doubling it
/// covers both without inflating the requirement further — an over-estimate here
/// refuses a migration that would have succeeded.
const REQUIRED_FREE_SPACE_MULTIPLIER: u64 = 2;

/// Reads as the tail of "RalphX needs N free to finish …" on the startup screen.
const MIGRATION_USER_FACING_OPERATION: &str = "upgrading the chat timeline";

pub fn migrate(conn: &Connection) -> AppResult<()> {
    ensure_sufficient_free_space(conn)?;

    let migration_result = (|| {
        conn.execute_batch("PRAGMA foreign_keys = OFF;")
            .map_err(|error| AppError::Database(error.to_string()))?;
        // Migration callbacks take `&Connection`, so `new_unchecked` is the
        // transaction guard available here. Its Drop implementation rolls back
        // if an error prevents `commit`. It is spelled out rather than using
        // `unchecked_transaction()` because that inherits the connection's
        // DEFERRED default, and this rebuild must keep `BEGIN IMMEDIATE` to
        // avoid a WAL write-upgrade surfacing as "database is locked".
        let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(|error| AppError::Database(error.to_string()))?;

        transaction
            .execute_batch(
        r#"
        CREATE TABLE chat_message_blocks_new (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            message_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
            run_id TEXT,
            sequence INTEGER NOT NULL,
            block_index INTEGER NOT NULL DEFAULT 0,
            role TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('text', 'tool_use', 'task', 'system_notice', 'error', 'thinking')),
            status TEXT NOT NULL CHECK (status IN ('streaming', 'finalized', 'error')),
            text TEXT,
            tool_call_id TEXT,
            tool_name TEXT,
            tool_status TEXT,
            tool_input_preview TEXT,
            tool_result_preview TEXT,
            metadata TEXT,
            provider_harness TEXT,
            provider_session_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            finalized_at TEXT,
            UNIQUE(conversation_id, sequence),
            UNIQUE(message_id, block_index)
        );

        INSERT INTO chat_message_blocks_new (
            id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
            text, tool_call_id, tool_name, tool_status, tool_input_preview, tool_result_preview,
            metadata, provider_harness, provider_session_id, created_at, updated_at, finalized_at
        )
        SELECT id, conversation_id, message_id, run_id, sequence, block_index, role, kind, status,
               text, tool_call_id, tool_name, tool_status, tool_input_preview, tool_result_preview,
               metadata, provider_harness, provider_session_id, created_at, updated_at, finalized_at
        FROM chat_message_blocks;

        DROP TABLE chat_message_blocks;
        ALTER TABLE chat_message_blocks_new RENAME TO chat_message_blocks;

        CREATE INDEX idx_chat_message_blocks_conversation_sequence
            ON chat_message_blocks(conversation_id, sequence DESC);
        CREATE INDEX idx_chat_message_blocks_message
            ON chat_message_blocks(message_id, block_index);
        CREATE INDEX idx_chat_message_blocks_tool_call
            ON chat_message_blocks(conversation_id, tool_call_id);
        -- v20260730000304 runs first, so DROP TABLE above takes its index with
        -- it. The payload retention prune batches on ORDER BY created_at +
        -- LIMIT and silently degrades to a full scan per batch without it.
        CREATE INDEX idx_chat_message_blocks_created_at
            ON chat_message_blocks(created_at);
        "#,
    )
            .map_err(|error| AppError::Database(error.to_string()))?;
        ensure_foreign_key_check_is_clean(&transaction)?;
        transaction
            .commit()
            .map_err(|error| AppError::Database(error.to_string()))
    })();

    // `foreign_keys` cannot be toggled inside the transaction. Always restore it
    // after the guard has either committed or rolled back on drop.
    let pragma_restore_result = conn
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| AppError::Database(error.to_string()));

    migration_result?;
    pragma_restore_result?;
    Ok(())
}

fn ensure_foreign_key_check_is_clean(transaction: &Transaction<'_>) -> AppResult<()> {
    let mut statement = transaction
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| AppError::Database(error.to_string()))?;
    let mut rows = statement
        .query([])
        .map_err(|error| AppError::Database(error.to_string()))?;

    if let Some(row) = rows
        .next()
        .map_err(|error| AppError::Database(error.to_string()))?
    {
        let table = row
            .get::<_, String>(0)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let parent = row
            .get::<_, String>(2)
            .map_err(|error| AppError::Database(error.to_string()))?;
        return Err(AppError::Database(format!(
            "chat message block rebuild left a foreign-key violation: {table} -> {parent}"
        )));
    }

    Ok(())
}

#[cfg(unix)]
fn ensure_sufficient_free_space(conn: &Connection) -> AppResult<()> {
    let Some(database_path) = conn.path().filter(|path| !path.is_empty()) else {
        return Ok(());
    };
    let Some(parent) = Path::new(database_path).parent() else {
        tracing::warn!("chat message block rebuild: skipping free-space preflight without a database parent directory");
        return Ok(());
    };
    let parent = match parent.canonicalize() {
        Ok(parent) if parent.is_absolute() => parent,
        Ok(_) => {
            tracing::warn!("chat message block rebuild: skipping free-space preflight for a non-absolute database directory");
            return Ok(());
        }
        Err(error) => {
            tracing::warn!(error = %error, "chat message block rebuild: skipping free-space preflight because the database directory could not be canonicalized");
            return Ok(());
        }
    };

    let page_count = conn
        .query_row("PRAGMA page_count", [], |row| row.get::<_, u64>(0))
        .map_err(|error| AppError::Database(error.to_string()))?;
    let page_size = conn
        .query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))
        .map_err(|error| AppError::Database(error.to_string()))?;
    let required_bytes = page_count
        .checked_mul(page_size)
        .and_then(|database_bytes| database_bytes.checked_mul(REQUIRED_FREE_SPACE_MULTIPLIER))
        .ok_or_else(|| {
            AppError::Database(
                "chat message block rebuild free-space requirement overflowed".to_string(),
            )
        })?;

    let filesystem = match statvfs(&parent) {
        Ok(filesystem) => filesystem,
        Err(error) => {
            tracing::warn!(error = %error, "chat message block rebuild: skipping free-space preflight because statvfs failed");
            return Ok(());
        }
    };
    let available_bytes = match u64::try_from(filesystem.blocks_available()).ok().and_then(
        |blocks| {
            u64::try_from(filesystem.fragment_size())
                .ok()
                .and_then(|fragment_size| blocks.checked_mul(fragment_size))
        },
    ) {
        Some(bytes) => bytes,
        None => {
            tracing::warn!("chat message block rebuild: skipping free-space preflight because statvfs values overflowed");
            return Ok(());
        }
    };

    check_free_space(required_bytes, available_bytes)
}

#[cfg(not(unix))]
fn ensure_sufficient_free_space(_conn: &Connection) -> AppResult<()> {
    Ok(())
}

/// Split from the `statvfs` probe so the refusal itself is testable without a
/// full disk.
pub(super) fn check_free_space(required_bytes: u64, available_bytes: u64) -> AppResult<()> {
    if available_bytes < required_bytes {
        // Typed rather than `Database(String)` so the startup failure surface can
        // recognize it and show the user an actionable message instead of the
        // generic "could not open its local workspace".
        return Err(AppError::InsufficientDiskSpace {
            operation: MIGRATION_USER_FACING_OPERATION.to_string(),
            required_bytes,
            available_bytes,
        });
    }

    Ok(())
}
