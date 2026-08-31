use std::path::PathBuf;
use std::time::Duration;

use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TypeDBDriver,
};

use crate::ledger::{self, schema_queries};
use crate::migration::{
    self, MigrationStatus, check_strict_order, list_migration_files, new_migration_path, status_rows,
};
use crate::url::TypeDbUrl;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Opts {
    pub url: TypeDbUrl,
    pub migrations_dir: PathBuf,
    pub schema_file: PathBuf,
    pub strict: bool,
    pub verbose: bool,
    pub wait_timeout: Option<Duration>,
}

pub struct Runner {
    opts: Opts,
    driver: Option<TypeDBDriver>,
}

impl Runner {
    pub fn new(opts: Opts) -> Self {
        Self { opts, driver: None }
    }

    async fn driver(&mut self) -> Result<&TypeDBDriver> {
        if self.driver.is_none() {
            if let Some(timeout) = self.opts.wait_timeout {
                wait_for_server(&self.opts.url, timeout, self.opts.verbose).await?;
            }
            self.driver = Some(open_driver(&self.opts.url).await?);
        }
        Ok(self.driver.as_ref().expect("driver set after connect"))
    }

    pub async fn create(&mut self) -> Result<()> {
        let name = self.opts.url.database.clone();
        let verbose = self.opts.verbose;
        let driver = self.driver().await?;
        if driver.databases().contains(name.clone()).await? {
            if verbose {
                eprintln!("database already exists: {name}");
            }
            return Ok(());
        }
        driver.databases().create(name.clone()).await?;
        println!("Created: {name}");
        Ok(())
    }

    pub async fn drop(&mut self) -> Result<()> {
        let name = self.opts.url.database.clone();
        let verbose = self.opts.verbose;
        let driver = self.driver().await?;
        if !driver.databases().contains(name.clone()).await? {
            if verbose {
                eprintln!("database does not exist: {name}");
            }
            return Ok(());
        }
        let db = driver.databases().get(name.clone()).await?;
        db.delete().await?;
        println!("Dropped: {name}");
        Ok(())
    }

    pub async fn new_migration(&self, name: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.opts.migrations_dir)?;
        let path = new_migration_path(&self.opts.migrations_dir, name);
        std::fs::write(&path, migration::migration_template())?;
        println!("Created: {}", path.display());
        Ok(path)
    }

    pub async fn migrate(&mut self) -> Result<()> {
        let db = self.opts.url.database.clone();
        let dir = self.opts.migrations_dir.clone();
        let strict = self.opts.strict;
        let verbose = self.opts.verbose;

        let driver = self.driver().await?;
        if !driver.databases().contains(db.clone()).await? {
            return Err(Error::msg(format!(
                "database does not exist: {db} (run create or up)"
            )));
        }
        ledger::ensure(driver, &db).await?;
        let files = list_migration_files(&dir)?;
        let applied = ledger::applied_versions(driver, &db).await?;
        if strict {
            check_strict_order(&files, &applied)?;
        }
        let pending: Vec<_> = files
            .into_iter()
            .filter(|f| !applied.iter().any(|v| v == &f.version))
            .collect();
        if pending.is_empty() {
            if verbose {
                eprintln!("Migrations: nothing to apply");
            }
            return Ok(());
        }
        for m in pending {
            apply_up(driver, &db, &m.version, &m.up, verbose).await?;
            println!("Applied: {}_{}", m.version, m.name);
        }
        Ok(())
    }

    pub async fn rollback(&mut self) -> Result<()> {
        let db = self.opts.url.database.clone();
        let dir = self.opts.migrations_dir.clone();
        let verbose = self.opts.verbose;

        let driver = self.driver().await?;
        ledger::ensure(driver, &db).await?;
        let files = list_migration_files(&dir)?;
        let mut applied = ledger::applied_versions(driver, &db).await?;
        let Some(version) = applied.pop() else {
            if verbose {
                eprintln!("Rollback: nothing to roll back");
            }
            return Ok(());
        };
        let m = files
            .iter()
            .find(|f| f.version == version)
            .ok_or_else(|| {
                Error::msg(format!(
                    "applied version {version} has no matching migration file"
                ))
            })?;
        if m.down.is_empty() {
            return Err(Error::msg(format!(
                "migration {}_{} has empty migrate:down",
                m.version, m.name
            )));
        }
        apply_down(driver, &db, &m.version, &m.down, verbose).await?;
        println!("Rolled back: {}_{}", m.version, m.name);
        Ok(())
    }

    pub async fn status(&mut self, quiet: bool) -> Result<bool> {
        let db = self.opts.url.database.clone();
        let dir = self.opts.migrations_dir.clone();
        let strict = self.opts.strict;

        let driver = self.driver().await?;
        let applied = if driver.databases().contains(db.clone()).await? {
            ledger::applied_versions(driver, &db)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let files = list_migration_files(&dir)?;
        if strict {
            check_strict_order(&files, &applied)?;
        }
        let rows = status_rows(&files, &applied);
        let pending = rows.iter().any(|(_, s)| *s == MigrationStatus::Pending);
        if !quiet {
            for (m, status) in &rows {
                let mark = match status {
                    MigrationStatus::Applied => "[X]",
                    MigrationStatus::Pending => "[ ]",
                };
                println!("{mark} {}_{}", m.version, m.name);
            }
            if rows.is_empty() {
                println!("No migrations found.");
            }
        }
        Ok(pending)
    }

    pub async fn dump(&mut self) -> Result<()> {
        let db_name = self.opts.url.database.clone();
        let schema_file = self.opts.schema_file.clone();
        let dir = self.opts.migrations_dir.clone();

        let driver = self.driver().await?;
        let db = driver.databases().get(db_name.clone()).await?;
        let schema = db.schema().await?;
        let applied = ledger::applied_versions(driver, &db_name)
            .await
            .unwrap_or_default();
        let files = list_migration_files(&dir)?;

        if let Some(parent) = schema_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = String::from("-- Schema dumped by tqlmate\n-- Applied migrations:\n");
        if applied.is_empty() {
            out.push_str("--   (none)\n");
        } else {
            for v in &applied {
                match files.iter().find(|f| &f.version == v) {
                    Some(m) => out.push_str(&format!("--   {}_{}\n", m.version, m.name)),
                    None => out.push_str(&format!("--   {v}\n")),
                }
            }
        }
        out.push('\n');
        out.push_str(schema.trim());
        out.push('\n');
        std::fs::write(&schema_file, out)?;
        println!("Wrote: {}", schema_file.display());
        Ok(())
    }

    pub async fn load(&mut self) -> Result<()> {
        let schema_file = self.opts.schema_file.clone();
        let db = self.opts.url.database.clone();
        let text = std::fs::read_to_string(&schema_file)?;
        let body = strip_dump_header(&text);
        if body.trim().is_empty() {
            return Err(Error::msg("schema file is empty"));
        }

        let driver = self.driver().await?;
        if !driver.databases().contains(db.clone()).await? {
            driver.databases().create(db.clone()).await?;
        }
        schema_queries(driver, &db, &[body.as_str()]).await?;
        println!("Loaded: {}", schema_file.display());
        Ok(())
    }

    pub async fn wait(&mut self, timeout: Duration) -> Result<()> {
        wait_for_server(&self.opts.url, timeout, self.opts.verbose).await
    }

    pub async fn up(&mut self) -> Result<()> {
        self.create().await?;
        self.migrate().await?;
        Ok(())
    }
}

async fn open_driver(url: &TypeDbUrl) -> Result<TypeDBDriver> {
    let addresses = Addresses::try_from_address_str(url.address())?;
    let credentials = Credentials::new(&url.username, &url.password);
    let tls = if url.tls {
        DriverTlsConfig::enabled_with_native_root_ca()
    } else {
        DriverTlsConfig::disabled()
    };
    Ok(TypeDBDriver::new(addresses, credentials, DriverOptions::new(tls)).await?)
}

async fn wait_for_server(url: &TypeDbUrl, timeout: Duration, verbose: bool) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        match open_driver(url).await {
            Ok(_) => {
                if verbose {
                    eprintln!("TypeDB available at {}", url.address());
                }
                return Ok(());
            }
            Err(e) => {
                if start.elapsed() >= timeout {
                    return Err(Error::msg(format!(
                        "timed out waiting for TypeDB at {}: {e}",
                        url.address()
                    )));
                }
                if verbose {
                    eprintln!("waiting for TypeDB at {} ({e})", url.address());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn apply_up(
    driver: &TypeDBDriver,
    database: &str,
    version: &str,
    up: &str,
    verbose: bool,
) -> Result<()> {
    if up.trim().is_empty() {
        return Err(Error::msg(format!("migration {version} has empty migrate:up")));
    }
    let applied_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let insert = ledger::record_insert(version, &applied_at);
    if verbose {
        eprintln!("-> up {version}");
    }
    schema_queries(driver, database, &[up, insert.as_str()]).await
}

async fn apply_down(
    driver: &TypeDBDriver,
    database: &str,
    version: &str,
    down: &str,
    verbose: bool,
) -> Result<()> {
    let delete = ledger::record_delete(version);
    if verbose {
        eprintln!("-> down {version}");
    }
    schema_queries(driver, database, &[down, delete.as_str()]).await
}

fn strip_dump_header(text: &str) -> String {
    text.lines()
        .skip_while(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with("--")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn resolve_url(cli_url: Option<&str>) -> Result<TypeDbUrl> {
    let raw = cli_url
        .map(str::to_string)
        .or_else(|| std::env::var("TYPEDB_URL").ok())
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| "typedb://admin:password@localhost:1729/typedb".into());
    TypeDbUrl::parse(&raw)
}

pub fn default_migrations_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("TQLMATE_MIGRATIONS_DIR").unwrap_or_else(|_| "db/migrations".into()),
    )
}

pub fn default_schema_file() -> PathBuf {
    PathBuf::from(std::env::var("TQLMATE_SCHEMA_FILE").unwrap_or_else(|_| "db/schema.tql".into()))
}
