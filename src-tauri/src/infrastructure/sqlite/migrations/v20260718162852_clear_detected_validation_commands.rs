// Migration v20260718162852: clear detected validation commands

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let analyses = {
        let mut statement = conn
            .prepare(
                "SELECT id, detected_analysis
                 FROM projects
                 WHERE detected_analysis IS NOT NULL",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;

        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| AppError::Database(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(error.to_string()))?;
        rows
    };

    for (project_id, raw_analysis) in analyses {
        let Ok(mut analysis) = serde_json::from_str::<Value>(&raw_analysis) else {
            continue;
        };
        let Some(entries) = analysis.as_array_mut() else {
            continue;
        };

        let mut changed = false;
        for entry in entries {
            let Some(entry) = entry.as_object_mut() else {
                continue;
            };
            if entry.get("validate") != Some(&Value::Array(Vec::new())) {
                entry.insert("validate".to_string(), Value::Array(Vec::new()));
                changed = true;
            }
        }

        if changed {
            let updated = serde_json::to_string(&analysis)
                .map_err(|error| AppError::Database(error.to_string()))?;
            conn.execute(
                "UPDATE projects SET detected_analysis = ?2 WHERE id = ?1",
                params![project_id, updated],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        }
    }

    Ok(())
}
