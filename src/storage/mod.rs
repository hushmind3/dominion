//! Product-only persistence; not linked into the portable agent kernel.
//! SQLite, data migrations and encrypted secrets stay behind host adapters.
pub mod sqlite;
pub mod migrations;
pub mod vault;
pub mod bounded_file;
