// Migration v20260509090000: persist last dismissed release notes version

use rusqlite::Connection;

use crate::error::AppResult;

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    add_column_if_not_exists(
        conn,
        "app_state",
        "last_seen_release_notes_version",
        "TEXT DEFAULT NULL",
    )
}
