// Migration v20260520150000: atlassian oauth settings

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE atlassian_integration_settings ADD COLUMN auth_method TEXT NOT NULL DEFAULT 'api_token'
            CHECK (auth_method IN ('api_token', 'oauth'));
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_client_id TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_redirect_uri TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_client_secret_ref TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_access_token_ref TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_refresh_token_ref TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_cloud_id TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_scopes TEXT;
        ALTER TABLE atlassian_integration_settings ADD COLUMN oauth_access_token_expires_at TEXT;
        "#,
    )?;
    Ok(())
}
