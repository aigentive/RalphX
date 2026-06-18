// Migration v20260521222911: agent plan mode

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute("PRAGMA legacy_alter_table = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    rewrite_table_check_constraint(
        conn,
        "chat_conversations",
        "'plan'",
        &[(
            "CHECK(agent_mode IN ('chat', 'edit', 'ideation'))",
            "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation'))",
        )],
        "agent Plan mode",
    )?;
    rewrite_table_check_constraint(
        conn,
        "agent_conversation_workspaces",
        "'plan'",
        &[
            (
                "CHECK (mode IN ('chat', 'edit', 'ideation'))",
                "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation'))",
            ),
            (
                "CHECK (mode IN ('edit', 'chat', 'ideation'))",
                "CHECK (mode IN ('edit', 'chat', 'plan', 'ideation'))",
            ),
            (
                "CHECK (mode IN ('edit', 'ideation', 'chat'))",
                "CHECK (mode IN ('edit', 'ideation', 'chat', 'plan'))",
            ),
        ],
        "agent Plan mode",
    )?;

    conn.execute(
        if legacy_alter_table_was_enabled {
            "PRAGMA legacy_alter_table = ON"
        } else {
            "PRAGMA legacy_alter_table = OFF"
        },
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute(
        if foreign_keys_was_enabled {
            "PRAGMA foreign_keys = ON"
        } else {
            "PRAGMA foreign_keys = OFF"
        },
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}

pub(super) fn foreign_keys_enabled(conn: &Connection) -> AppResult<bool> {
    conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
        .map(|value| value != 0)
        .map_err(|error| AppError::Database(error.to_string()))
}

pub(super) fn legacy_alter_table_enabled(conn: &Connection) -> AppResult<bool> {
    conn.query_row("PRAGMA legacy_alter_table", [], |row| row.get::<_, i64>(0))
        .map(|value| value != 0)
        .map_err(|error| AppError::Database(error.to_string()))
}

pub(super) fn rewrite_table_check_constraint(
    conn: &Connection,
    table_name: &'static str,
    already_allowed_value: &str,
    replacements: &[(&str, &str)],
    error_label: &str,
) -> AppResult<()> {
    let create_sql = table_create_sql(conn, table_name)?;
    if create_sql.contains(already_allowed_value) {
        return Ok(());
    }

    let replacement_sql = apply_replacements(&create_sql, replacements).ok_or_else(|| {
        AppError::Database(format!(
            "Could not find {error_label} CHECK constraint for {table_name}"
        ))
    })?;
    let new_table_name = format!("{table_name}_new_plan_mode");
    let old_table_name = format!("{table_name}_old_plan_mode");
    let new_table_sql = rename_create_table_sql(&replacement_sql, table_name, &new_table_name)?;
    let columns = table_columns(conn, table_name)?;
    let column_list = columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let dependent_objects = dependent_object_sql(conn, table_name)?;

    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS {new_table};
         DROP TABLE IF EXISTS {old_table};
         ALTER TABLE {table} RENAME TO {old_table};
         {new_table_sql};
         INSERT INTO {new_table} ({column_list})
         SELECT {column_list} FROM {old_table};
         DROP TABLE {old_table};
         ALTER TABLE {new_table} RENAME TO {table};",
        table = quote_identifier(table_name),
        new_table = quote_identifier(&new_table_name),
        old_table = quote_identifier(&old_table_name),
    ))
    .map_err(|error| AppError::Database(error.to_string()))?;

    for sql in dependent_objects {
        conn.execute_batch(&sql)
            .map_err(|error| AppError::Database(error.to_string()))?;
    }

    Ok(())
}

fn table_create_sql(conn: &Connection, table_name: &str) -> AppResult<String> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table_name],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(error.to_string()))
}

fn apply_replacements(sql: &str, replacements: &[(&str, &str)]) -> Option<String> {
    let mut updated = sql.to_string();
    let mut replaced = false;
    for (from, to) in replacements {
        if updated.contains(from) {
            updated = updated.replace(from, to);
            replaced = true;
        }
    }
    replaced.then_some(updated)
}

fn rename_create_table_sql(
    sql: &str,
    original_table_name: &str,
    new_table_name: &str,
) -> AppResult<String> {
    let original = format!("CREATE TABLE {original_table_name}");
    if sql.contains(&original) {
        return Ok(sql.replacen(&original, &format!("CREATE TABLE {new_table_name}"), 1));
    }

    let quoted_original = format!("CREATE TABLE {}", quote_identifier(original_table_name));
    if sql.contains(&quoted_original) {
        return Ok(sql.replacen(
            &quoted_original,
            &format!("CREATE TABLE {}", quote_identifier(new_table_name)),
            1,
        ));
    }

    Err(AppError::Database(format!(
        "Could not rewrite CREATE TABLE statement for {original_table_name}"
    )))
}

fn table_columns(conn: &Connection, table_name: &str) -> AppResult<Vec<String>> {
    let mut statement = conn
        .prepare(&format!(
            "PRAGMA table_info({})",
            quote_identifier(table_name)
        ))
        .map_err(|error| AppError::Database(error.to_string()))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| AppError::Database(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(error.to_string()))?;
    if columns.is_empty() {
        return Err(AppError::Database(format!(
            "Table {table_name} has no columns"
        )));
    }
    Ok(columns)
}

fn dependent_object_sql(conn: &Connection, table_name: &str) -> AppResult<Vec<String>> {
    let mut statement = conn
        .prepare(
            "SELECT sql
             FROM sqlite_master
             WHERE tbl_name = ?1
               AND type IN ('index', 'trigger')
               AND sql IS NOT NULL
             ORDER BY type, name",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    let objects = statement
        .query_map([table_name], |row| row.get::<_, String>(0))
        .map_err(|error| AppError::Database(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(objects)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
