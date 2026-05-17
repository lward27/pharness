#![forbid(unsafe_code)]

mod models;
mod sqlite;

pub use models::{
    CreateApproval, CreateArtifact, CreateFileChange, CreateRun, CreateSession, StoredApproval,
    StoredArtifact, StoredFileChange, StoredRun,
};
pub use sqlite::{SqliteStore, StoreError};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";
