use super::*;

fn create_first_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("CREATE TABLE progress_first (id INTEGER PRIMARY KEY)")?;
    Ok(())
}

fn create_second_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("CREATE TABLE progress_second (id INTEGER PRIMARY KEY)")?;
    Ok(())
}

fn fail_second_migration(_conn: &Connection) -> AppResult<()> {
    Err(AppError::Database("injected migration failure".to_string()))
}

const TWO_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "progress_first",
        migrate: create_first_table,
    },
    Migration {
        version: 2,
        name: "progress_second",
        migrate: create_second_table,
    },
];

#[test]
fn zero_pending_migrations_report_real_zero_progress() {
    let conn = Connection::open_in_memory().unwrap();
    let mut progress = Vec::new();

    run_pending_migrations(&conn, &[], |event| progress.push(event)).unwrap();

    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].completed_units, 0);
    assert_eq!(progress[0].total_units, 0);
}

#[test]
fn multiple_pending_migrations_report_monotonic_real_units() {
    let conn = Connection::open_in_memory().unwrap();
    let mut progress = Vec::new();

    run_pending_migrations(&conn, TWO_MIGRATIONS, |event| progress.push(event)).unwrap();

    assert_eq!(
        progress
            .iter()
            .map(|event| (event.completed_units, event.total_units))
            .collect::<Vec<_>>(),
        vec![(0, 2), (1, 2), (2, 2)]
    );
    assert!(progress
        .windows(2)
        .all(|pair| pair[0].elapsed_ms <= pair[1].elapsed_ms));
}

#[test]
fn failed_migration_never_reports_false_completion() {
    let conn = Connection::open_in_memory().unwrap();
    let migrations = [
        TWO_MIGRATIONS[0],
        Migration {
            version: 2,
            name: "progress_failure",
            migrate: fail_second_migration,
        },
    ];
    let mut progress = Vec::new();

    let error =
        run_pending_migrations(&conn, &migrations, |event| progress.push(event)).unwrap_err();

    assert!(error.to_string().contains("injected migration failure"));
    assert_eq!(
        progress
            .iter()
            .map(|event| (event.completed_units, event.total_units))
            .collect::<Vec<_>>(),
        vec![(0, 2), (1, 2)]
    );
    let applied_versions = get_applied_migration_versions(&conn).unwrap();
    assert!(applied_versions.contains(&1));
    assert!(!applied_versions.contains(&2));
}
