//! Database migrations
//!
//! Handles schema creation and updates.

use rusqlite::Connection;

/// The current schema SQL
const SCHEMA_SQL: &str = include_str!("../../migrations/001_initial.sql");

/// Run all pending migrations
pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Create migrations table if it doesn't exist
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at INTEGER NOT NULL
        )",
        [],
    )?;

    // Check if initial migration has been applied
    let applied: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM _migrations WHERE name = '001_initial'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !applied {
        tracing::info!("Applying migration: 001_initial");

        // Execute the schema SQL
        conn.execute_batch(SCHEMA_SQL)?;

        // Record the migration
        conn.execute(
            "INSERT INTO _migrations (name, applied_at) VALUES ('001_initial', ?)",
            [chrono::Utc::now().timestamp_millis()],
        )?;

        tracing::info!("Migration 001_initial applied successfully");
    }

    Ok(())
}

/// Get the list of applied migrations
pub fn get_applied_migrations(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT name FROM _migrations ORDER BY id")?;
    let names = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(names)
}

/// Check if the database needs migrations
pub fn needs_migration(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let applied = get_applied_migrations(conn)?;
    // For now, we only have one migration
    Ok(!applied.contains(&"001_initial".to_string()))
}
