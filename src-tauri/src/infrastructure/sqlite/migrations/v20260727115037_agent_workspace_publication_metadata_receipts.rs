// Migration v20260727115037: agent workspace publication metadata receipts

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_metadata_phase",
        "TEXT NULL",
    )?;
    for column in [
        "publication_metadata_target_pr_number",
        "publication_metadata_before_authority_sha256",
        "publication_metadata_before_title_sha256",
        "publication_metadata_before_editable_body_sha256",
        "publication_metadata_before_managed_suffix_sha256",
        "publication_metadata_intended_title_sha256",
        "publication_metadata_intended_editable_body_sha256",
        "publication_metadata_updated_at",
    ] {
        helpers::add_column_if_not_exists(
            conn,
            "agent_conversation_workspaces",
            column,
            if column == "publication_metadata_target_pr_number" {
                "INTEGER NULL"
            } else {
                "TEXT NULL"
            },
        )?;
    }
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_metadata_state",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspaces",
        "publication_metadata_attempt_id",
        "TEXT NULL",
    )?;
    helpers::add_column_if_not_exists(
        conn,
        "agent_conversation_workspace_publication_events",
        "attempt_id",
        "TEXT NULL",
    )?;
    Ok(())
}
