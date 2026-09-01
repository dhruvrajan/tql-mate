use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Digit-only migration version (lexicographic order matches chronological timestamps).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(String);

impl Version {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Version {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Version {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Version {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFile {
    pub version: Version,
    pub name: String,
    pub path: PathBuf,
    pub up: String,
    pub down: String,
}

impl MigrationFile {
    pub fn label(&self) -> String {
        format!("{}_{}", self.version, self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Applied,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Up,
    Down,
}

pub fn parse_version_name(filename: &str) -> Result<(Version, String)> {
    let stem = filename
        .strip_suffix(".tql")
        .ok_or_else(|| Error::MigrationExtension(filename.to_string()))?;
    let (version, rest) = stem
        .split_once('_')
        .ok_or_else(|| Error::MigrationFilename(filename.to_string()))?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::MigrationVersionDigits(filename.to_string()));
    }
    if rest.is_empty() {
        return Err(Error::MigrationName(filename.to_string()));
    }
    Ok((Version::new(version), rest.to_string()))
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

/// Split migration file text into `(up, down)` bodies (no TypeDB I/O).
pub fn parse_migration_body(text: &str) -> Result<(String, String)> {
    split_up_down(text)
}

fn split_up_down(text: &str) -> Result<(String, String)> {
    let mut section = None::<Section>;
    let mut up = String::new();
    let mut down = String::new();

    for line in text.lines() {
        if let Some(marker) = migration_marker(line) {
            section = Some(marker);
            continue;
        }
        match section {
            Some(Section::Up) => {
                up.push_str(line);
                up.push('\n');
            }
            Some(Section::Down) => {
                down.push_str(line);
                down.push('\n');
            }
            None => {}
        }
    }

    if section.is_none() && up.is_empty() && down.is_empty() {
        return Err(Error::MigrationMarkers);
    }
    Ok((up.trim().to_string(), down.trim().to_string()))
}

fn migration_marker(line: &str) -> Option<Section> {
    let marker = line.trim().strip_prefix("--")?.trim();
    if marker.eq_ignore_ascii_case("migrate:up") {
        Some(Section::Up)
    } else if marker.eq_ignore_ascii_case("migrate:down") {
        Some(Section::Down)
    } else {
        None
    }
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
            return Err(Error::DuplicateVersion(files[i].version.clone()));
        }
    }
    Ok(files)
}

pub fn status_rows(
    files: &[MigrationFile],
    applied: &[Version],
) -> Vec<(MigrationFile, MigrationStatus)> {
    let applied: HashSet<&Version> = applied.iter().collect();
    files
        .iter()
        .cloned()
        .map(|f| {
            let status = if applied.contains(&f.version) {
                MigrationStatus::Applied
            } else {
                MigrationStatus::Pending
            };
            (f, status)
        })
        .collect()
}

pub fn check_strict_order(files: &[MigrationFile], applied: &[Version]) -> Result<()> {
    let Some(max) = applied.iter().max() else {
        return Ok(());
    };
    for f in files {
        if !applied.iter().any(|v| v == &f.version) && f.version.as_str() < max.as_str() {
            return Err(Error::StrictOrder {
                pending: f.version.clone(),
                applied_up_to: max.clone(),
            });
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

pub fn slugify(name: &str) -> String {
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
