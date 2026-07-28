//! Tests for migration v20260728162405: rx native team runtime

use rusqlite::Connection;

use super::{helpers, run_migrations_through, v20260728162405_rx_native_team_runtime};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_creates_the_durable_team_runtime_schema() {
    let conn = setup_test_db();
    run_migrations_through(&conn, 20260727115037).expect("prior migrations should succeed");
    v20260728162405_rx_native_team_runtime::migrate(&conn).unwrap();

    for table in [
        "managed_team_sessions",
        "managed_team_members",
        "managed_team_run_bindings",
        "managed_team_messages",
        "managed_team_message_deliveries",
        "managed_team_wake_batches",
        "managed_team_workspace_reservations",
    ] {
        assert!(helpers::table_exists(&conn, table), "missing {table}");
    }
    for column in ["team_id", "team_member_id", "team_member_generation"] {
        assert!(helpers::column_exists(
            &conn,
            "agent_task_delegate_assignments",
            column
        ));
    }
}
