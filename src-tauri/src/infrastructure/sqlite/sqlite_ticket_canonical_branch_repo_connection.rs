use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteTicketCanonicalBranchRepository {
    pub(super) db: DbConnection,
}

impl SqliteTicketCanonicalBranchRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}
