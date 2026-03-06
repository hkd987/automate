use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
}

pub fn init_state(conn: Connection) -> AppState {
    AppState {
        db: Arc::new(Mutex::new(conn)),
    }
}
