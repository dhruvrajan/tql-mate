use futures::StreamExt;
use typedb_driver::{TransactionType, TypeDBDriver};

use crate::migration::{MigrationFile, Version};
use crate::{Error, Result};

pub const ENTITY: &str = "_tqlmate_schema_migration";
pub const ATTR_VERSION: &str = "_tqlmate_version";
pub const ATTR_APPLIED_AT: &str = "_tqlmate_applied_at";

const ENSURE_SCHEMA: &str = concat!(
    "define\n",
    "  attribute _tqlmate_version, value string;\n",
    "  attribute _tqlmate_applied_at, value datetime;\n",
    "  entity _tqlmate_schema_migration,\n",
    "    owns _tqlmate_version @card(1),\n",
    "    owns _tqlmate_applied_at @card(1);\n",
);

/// Bootstrap TypeQL that creates the `_tqlmate_*` ledger types (pure string; no network).
pub fn bootstrap_schema() -> &'static str {
    ENSURE_SCHEMA
}

pub async fn ensure(driver: &TypeDBDriver, database: &str) -> Result<()> {
    if applied_versions(driver, database).await.is_ok() {
        return Ok(());
    }
    schema_queries(driver, database, &[bootstrap_schema()]).await
}

pub async fn applied_versions(driver: &TypeDBDriver, database: &str) -> Result<Vec<Version>> {
    let tx = driver.transaction(database, TransactionType::Read).await?;
    let query = format!("match $m isa {ENTITY}, has {ATTR_VERSION} $v;");
    let answer = match tx.query(query).await {
        Ok(a) => a,
        Err(e) => {
            let _ = tx.close().await;
            return Err(e.into());
        }
    };
    let mut versions = Vec::new();
    let mut rows = answer.into_rows();
    while let Some(row) = rows.next().await {
        let row = row?;
        let concept = row
            .get("v")?
            .ok_or_else(|| Error::msg("missing version column"))?;
        let version = concept
            .try_get_string()
            .ok_or_else(|| Error::msg("version is not a string"))?
            .to_string();
        versions.push(Version::new(version));
    }
    let _ = tx.close().await;
    versions.sort();
    Ok(versions)
}

pub fn record_insert(version: &Version, applied_at: &str) -> String {
    format!(
        "insert $_ isa {ENTITY}, has {ATTR_VERSION} \"{}\", has {ATTR_APPLIED_AT} {applied_at};",
        version.as_str()
    )
}

pub fn record_delete(version: &Version) -> String {
    format!(
        "match $m isa {ENTITY}, has {ATTR_VERSION} \"{}\"; delete $m;",
        version.as_str()
    )
}

/// Header comments written by `dump`, listing applied migration labels.
pub fn dump_header(applied: &[Version], files: &[MigrationFile]) -> String {
    let mut out = String::from("-- Schema dumped by tqlmate\n-- Applied migrations:\n");
    if applied.is_empty() {
        out.push_str("--   (none)\n");
    } else {
        for v in applied {
            match files.iter().find(|f| &f.version == v) {
                Some(m) => out.push_str(&format!("--   {}\n", m.label())),
                None => out.push_str(&format!("--   {v}\n")),
            }
        }
    }
    out.push('\n');
    out
}

pub fn strip_dump_header(text: &str) -> String {
    text.lines()
        .skip_while(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with("--")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn schema_queries(driver: &TypeDBDriver, database: &str, queries: &[&str]) -> Result<()> {
    let tx = driver
        .transaction(database, TransactionType::Schema)
        .await?;
    for q in queries {
        let trimmed = q.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Err(e) = tx.query(trimmed).await {
            let _ = tx.close().await;
            return Err(e.into());
        }
    }
    tx.commit().await?;
    Ok(())
}
