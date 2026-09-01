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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ledger_typeql_insert_delete() {
        let v = Version::new("20240101120000");
        assert_eq!(
            record_insert(&v, "2024-01-01T12:00:00"),
            "insert $_ isa _tqlmate_schema_migration, has _tqlmate_version \"20240101120000\", has _tqlmate_applied_at 2024-01-01T12:00:00;"
        );
        assert_eq!(
            record_delete(&v),
            "match $m isa _tqlmate_schema_migration, has _tqlmate_version \"20240101120000\"; delete $m;"
        );
    }

    #[test]
    fn bootstrap_schema_defines_ledger_types() {
        let schema = bootstrap_schema();
        assert!(schema.contains(ENTITY));
        assert!(schema.contains(ATTR_VERSION));
        assert!(schema.contains(ATTR_APPLIED_AT));
        assert!(schema.contains("@card(1)"));
        assert!(schema.trim_start().starts_with("define"));
    }

    #[test]
    fn dump_header_lists_labels_or_raw_versions() {
        let files = [MigrationFile {
            version: Version::new("1"),
            name: "person".into(),
            path: PathBuf::from("1_person.tql"),
            up: String::new(),
            down: String::new(),
        }];
        let empty = dump_header(&[], &files);
        assert!(empty.contains("-- Schema dumped by tqlmate"));
        assert!(empty.contains("--   (none)"));
        assert!(empty.ends_with('\n'));

        let with = dump_header(&[Version::new("1"), Version::new("99")], &files);
        assert!(with.contains("--   1_person\n"));
        assert!(with.contains("--   99\n"));
    }

    #[test]
    fn strip_dump_header_removes_comment_preamble() {
        let text = "-- Schema dumped by tqlmate\n-- Applied migrations:\n--   1_person\n\ndefine\n  entity x;\n";
        assert_eq!(strip_dump_header(text), "define\n  entity x;");
        assert_eq!(strip_dump_header("define entity y;"), "define entity y;");
        assert_eq!(strip_dump_header("-- only comments\n\n"), "");
    }

    #[test]
    fn dump_header_round_trip_with_strip() {
        let files = [MigrationFile {
            version: Version::new("20240101000000"),
            name: "person".into(),
            path: PathBuf::from("20240101000000_person.tql"),
            up: String::new(),
            down: String::new(),
        }];
        let header = dump_header(&[Version::new("20240101000000")], &files);
        let full = format!("{header}define\n  entity person;\n");
        assert_eq!(strip_dump_header(&full), "define\n  entity person;");
        assert!(bootstrap_schema().contains(ENTITY));
    }
}
