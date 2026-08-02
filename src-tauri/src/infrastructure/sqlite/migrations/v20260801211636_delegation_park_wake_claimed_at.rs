// Migration v20260801211636: delegation park wake claimed at

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "delegation_parks", "wake_claimed_at", "TEXT")
}
