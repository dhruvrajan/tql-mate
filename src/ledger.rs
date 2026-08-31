use futures::StreamExt;
use typedb_driver::{TransactionType, TypeDBDriver};

use crate::{Error, Result};

pub(crate) const ENTITY: &str = "_tqlmate_schema_migration";
pub(crate) const ATTR_VERSION: &str = "_tqlmate_version";
pub(crate) const ATTR_APPLIED_AT: &str = "_tqlmate_applied_at";

const ENSURE_SCHEMA: &str = concat!(
    "define\n",
    "  attribute _tqlmate_version, value string;\n",
    "  attribute _tqlmate_applied_at, value datetime;\n",
    "  entity _tqlmate_schema_migration,\n",
    "    owns _tqlmate_version @card(1),\n",
    "    owns _tqlmate_applied_at @card(1);\n",
);

pub async fn ensure(driver: &TypeDBDriver, database: &str) -> Result<()> {
    if applied_versions(driver, database).await.is_ok() {
        return Ok(());
    }
    let tx = driver
        .transaction(database, TransactionType::Schema)
        .await?;
    if let Err(e) = tx.query(ENSURE_SCHEMA).await {
        let _ = tx.close().await;
        return Err(e.into());
    }
    tx.commit().await?;
    Ok(())
}

pub async fn applied_versions(driver: &TypeDBDriver, database: &str) -> Result<Vec<String>> {
    let tx = driver
        .transaction(database, TransactionType::Read)
        .await?;
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
        versions.push(version);
    }
    let _ = tx.close().await;
    versions.sort();
    Ok(versions)
}

pub fn record_insert(version: &str, applied_at: &str) -> String {
    format!(
        "insert $_ isa {ENTITY}, has {ATTR_VERSION} \"{version}\", has {ATTR_APPLIED_AT} {applied_at};"
    )
}

pub fn record_delete(version: &str) -> String {
    format!("match $m isa {ENTITY}, has {ATTR_VERSION} \"{version}\"; delete $m;")
}

pub async fn schema_queries(driver: &TypeDBDriver, database: &str, queries: &[&str]) -> Result<()> {
    let tx = driver
        .transaction(database, TransactionType::Schema)
        .await?;
    for q in queries {
        if let Err(e) = tx.query(*q).await {
            let _ = tx.close().await;
            return Err(e.into());
        }
    }
    tx.commit().await?;
    Ok(())
}
