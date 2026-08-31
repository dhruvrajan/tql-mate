use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFile {
    pub version: String,
    pub name: String,
    pub path: PathBuf,
    pub up: String,
    pub down: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Applied,
    Pending,
}

pub fn parse_version_name(filename: &str) -> Result<(String, String)> {
    let stem = filename
        .strip_suffix(".tql")
        .ok_or_else(|| Error::msg(format!("migration must end in .tql: {filename}")))?;
    let (version, rest) = stem
        .split_once('_')
        .ok_or_else(|| Error::msg(format!("expected VERSION_name.tql: {filename}")))?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::msg(format!("version must be digits: {filename}")));
    }
    if rest.is_empty() {
        return Err(Error::msg(format!("missing name after version: {filename}")));
    }
    Ok((version.to_string(), rest.to_string()))
}

pub fn parse_migration(path: &Path) -> Result<MigrationFile> {
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::msg("invalid migration filename"))?;
    let (version, name) = parse_version_name(filename)?;
    let text = fs::read_to_string(path)?;
    let (up, down) = split_up_down(&text)?;
    Ok(MigrationFile {
        version,
        name,
        path: path.to_path_buf(),
        up,
        down,
    })
}

fn split_up_down(text: &str) -> Result<(String, String)> {
    let mut section = None::<&str>;
    let mut up = String::new();
    let mut down = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        let marker = trimmed
            .strip_prefix("--")
            .map(str::trim)
            .unwrap_or("");
        if marker.eq_ignore_ascii_case("migrate:up") {
            section = Some("up");
            continue;
        }
        if marker.eq_ignore_ascii_case("migrate:down") {
            section = Some("down");
            continue;
        }
        match section {
            Some("up") => {
                up.push_str(line);
                up.push('\n');
            }
            Some("down") => {
                down.push_str(line);
                down.push('\n');
            }
            _ => {}
        }
    }

    if section.is_none() && up.is_empty() && down.is_empty() {
        return Err(Error::msg("migration missing -- migrate:up / -- migrate:down"));
    }
    Ok((up.trim().to_string(), down.trim().to_string()))
}

pub fn list_migration_files(dir: &Path) -> Result<Vec<MigrationFile>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("tql") {
            continue;
        }
        files.push(parse_migration(&path)?);
    }
    files.sort_by(|a, b| a.version.cmp(&b.version));
    for i in 1..files.len() {
        if files[i].version == files[i - 1].version {
            return Err(Error::msg(format!(
                "duplicate migration version {}",
                files[i].version
            )));
        }
    }
    Ok(files)
}

pub fn status_rows(
    files: &[MigrationFile],
    applied: &[String],
) -> Vec<(MigrationFile, MigrationStatus)> {
    files
        .iter()
        .cloned()
        .map(|f| {
            let status = if applied.iter().any(|v| v == &f.version) {
                MigrationStatus::Applied
            } else {
                MigrationStatus::Pending
            };
            (f, status)
        })
        .collect()
}

pub fn check_strict_order(files: &[MigrationFile], applied: &[String]) -> Result<()> {
    let max_applied = applied.iter().max();
    if let Some(max) = max_applied {
        for f in files {
            if !applied.iter().any(|v| v == &f.version) && f.version.as_str() < max.as_str() {
                return Err(Error::msg(format!(
                    "strict: pending migration {} would apply out of order (applied up to {max})",
                    f.version
                )));
            }
        }
    }
    Ok(())
}

pub fn new_migration_path(dir: &Path, name: &str) -> PathBuf {
    let slug = slugify(name);
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    dir.join(format!("{ts}_{slug}.tql"))
}

pub fn migration_template() -> &'static str {
    "-- migrate:up\n\n\n-- migrate:down\n\n"
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "migration".into()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn file_naming() {
        let (v, n) = parse_version_name("20240101120000_create_person.tql").unwrap();
        assert_eq!(v, "20240101120000");
        assert_eq!(n, "create_person");
        assert!(parse_version_name("nope.tql").is_err());
        assert!(parse_version_name("abc_name.tql").is_err());
    }

    #[test]
    fn parses_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20240101120000_demo.tql");
        let mut f = fs::File::create(&path).unwrap();
        write!(
            f,
            "-- migrate:up\ndefine entity x;\n\n-- migrate:down\nundefine entity x;\n"
        )
        .unwrap();
        let m = parse_migration(&path).unwrap();
        assert_eq!(m.up, "define entity x;");
        assert_eq!(m.down, "undefine entity x;");
    }

    #[test]
    fn ordering_and_strict() {
        let a = MigrationFile {
            version: "1".into(),
            name: "a".into(),
            path: "a.tql".into(),
            up: String::new(),
            down: String::new(),
        };
        let b = MigrationFile {
            version: "2".into(),
            name: "b".into(),
            path: "b.tql".into(),
            up: String::new(),
            down: String::new(),
        };
        let c = MigrationFile {
            version: "3".into(),
            name: "c".into(),
            path: "c.tql".into(),
            up: String::new(),
            down: String::new(),
        };
        let files = vec![a.clone(), b.clone(), c.clone()];
        let rows = status_rows(&files, &["1".into(), "3".into()]);
        assert_eq!(rows[0].1, MigrationStatus::Applied);
        assert_eq!(rows[1].1, MigrationStatus::Pending);
        assert_eq!(rows[2].1, MigrationStatus::Applied);
        assert!(check_strict_order(&files, &["1".into(), "3".into()]).is_err());
        assert!(check_strict_order(&files, &["1".into(), "2".into()]).is_ok());
    }

    #[test]
    fn lists_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["20240201000000_b.tql", "20240101000000_a.tql"] {
            let mut f = fs::File::create(dir.path().join(name)).unwrap();
            write!(f, "-- migrate:up\n\n-- migrate:down\n").unwrap();
        }
        let files = list_migration_files(dir.path()).unwrap();
        assert_eq!(files[0].version, "20240101000000");
        assert_eq!(files[1].version, "20240201000000");
    }
}
