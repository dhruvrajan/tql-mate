mod ledger;
mod migration;
mod runner;
mod url;

pub use migration::{
    MigrationFile, MigrationStatus, check_strict_order, list_migration_files, parse_migration,
    parse_version_name, status_rows,
};
pub use runner::{
    Opts, Runner, default_migrations_dir, default_schema_file, resolve_url,
};
pub use url::TypeDbUrl;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    TypeDb(#[from] typedb_driver::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}
