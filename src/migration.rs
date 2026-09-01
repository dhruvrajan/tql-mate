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

    fn mf(version: &str) -> MigrationFile {
        MigrationFile {
            version: Version::new(version),
            name: version.into(),
            path: format!("{version}.tql").into(),
            up: String::new(),
            down: String::new(),
        }
    }

    #[test]
    fn version_name_table() {
        let ok = [
            (
                "20240101120000_create_person.tql",
                "20240101120000",
                "create_person",
            ),
            ("1_a.tql", "1", "a"),
            ("000_has_underscores_ok.tql", "000", "has_underscores_ok"),
        ];
        for (filename, ver, name) in ok {
            let (v, n) = parse_version_name(filename).unwrap();
            assert_eq!(v.as_str(), ver);
            assert_eq!(n, name);
        }

        let bad = [
            ("nope.tql", "MigrationFilename"),
            ("abc_name.tql", "MigrationVersionDigits"),
            ("123_.tql", "MigrationName"),
            ("_name.tql", "MigrationVersionDigits"),
            ("123_name.sql", "MigrationExtension"),
            ("123_name", "MigrationExtension"),
        ];
        for (filename, kind) in bad {
            let err = parse_version_name(filename).unwrap_err();
            let label = format!("{err:?}");
            assert!(
                label.contains(kind),
                "{filename}: expected {kind} in {label}"
            );
        }
    }

    #[test]
    fn parser_edge_cases() {
        let ok: &[(&str, &str, &str)] = &[
            (
                "-- migrate:up\ndefine entity x;\n\n-- migrate:down\nundefine entity x;\n",
                "define entity x;",
                "undefine entity x;",
            ),
            (
                "-- MIGRATE:UP\ndefine entity x;\n-- Migrate:Down\nundefine entity x;\n",
                "define entity x;",
                "undefine entity x;",
            ),
            (
                "  -- migrate:up  \ndefine entity x;\n  -- migrate:down\nundefine entity x;\n",
                "define entity x;",
                "undefine entity x;",
            ),
            ("-- migrate:up\ndefine entity x;\n", "define entity x;", ""),
            (
                "-- migrate:down\nundefine entity x;\n",
                "",
                "undefine entity x;",
            ),
            (
                "-- migrate:down\nundefine entity x;\n-- migrate:up\ndefine entity x;\n",
                "define entity x;",
                "undefine entity x;",
            ),
            (
                "-- preamble\n-- migrate:up\ndefine entity x;\n-- migrate:down\n\n",
                "define entity x;",
                "",
            ),

            (
                "-- migrate:up\r\ndefine entity x;\r\n-- migrate:down\r\nundefine entity x;\r\n",
                "define entity x;",
                "undefine entity x;",
            ),
            (
                "junk before\n-- migrate:up\ndefine entity x;\nextra still in up\n-- migrate:down\nundefine entity x;\n",
                "define entity x;\nextra still in up",
                "undefine entity x;",
            ),
        ];
        for (text, eu, ed) in ok {
            let (u, d) = split_up_down(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!((u.as_str(), d.as_str()), (*eu, *ed), "text={text:?}");
        }

        for text in ["no markers here", ""] {
            assert!(
                matches!(split_up_down(text), Err(Error::MigrationMarkers)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn parse_migration_from_disk() {
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
        assert_eq!(m.label(), "20240101120000_demo");
    }

    #[test]
    fn status_and_strict_ordering() {
        let files = vec![mf("1"), mf("2"), mf("3")];
        let applied = [Version::new("1"), Version::new("3")];
        let rows = status_rows(&files, &applied);
        assert_eq!(rows[0].1, MigrationStatus::Applied);
        assert_eq!(rows[1].1, MigrationStatus::Pending);
        assert_eq!(rows[2].1, MigrationStatus::Applied);

        assert!(matches!(
            check_strict_order(&files, &applied),
            Err(Error::StrictOrder {
                pending,
                applied_up_to
            }) if pending.as_str() == "2" && applied_up_to.as_str() == "3"
        ));
        assert!(check_strict_order(&files, &[Version::new("1"), Version::new("2")]).is_ok());
        assert!(check_strict_order(&files, &[]).is_ok());
        assert!(check_strict_order(
            &files,
            &[Version::new("1"), Version::new("2"), Version::new("3")]
        )
        .is_ok());

        // Pending after max is fine; only lower pending fails.
        assert!(check_strict_order(&files, &[Version::new("1")]).is_ok());
    }

    #[test]
    fn lists_sorted_and_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["20240201000000_b.tql", "20240101000000_a.tql"] {
            let mut f = fs::File::create(dir.path().join(name)).unwrap();
            write!(f, "-- migrate:up\n\n-- migrate:down\n").unwrap();
        }
        let files = list_migration_files(dir.path()).unwrap();
        assert_eq!(files[0].version.as_str(), "20240101000000");
        assert_eq!(files[1].version.as_str(), "20240201000000");

        let mut f = fs::File::create(dir.path().join("20240101000000_dup.tql")).unwrap();
        write!(f, "-- migrate:up\n\n-- migrate:down\n").unwrap();
        assert!(matches!(
            list_migration_files(dir.path()),
            Err(Error::DuplicateVersion(v)) if v.as_str() == "20240101000000"
        ));
    }

    #[test]
    fn empty_dir_and_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_migration_files(dir.path()).unwrap().is_empty());
        assert!(list_migration_files(&dir.path().join("missing"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn parser_handles_crlf_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20240101120000_crlf.tql");
        std::fs::write(
            &path,
            "-- migrate:up\r\ndefine entity crlf;\r\n\r\n-- migrate:down\r\nundefine entity crlf;\r\n",
        )
        .unwrap();
        let m = parse_migration(&path).unwrap();
        assert_eq!(m.up, "define entity crlf;");
        assert_eq!(m.down, "undefine entity crlf;");
    }

    #[test]
    fn slugify_cases() {
        assert_eq!(slugify("Create Person"), "create_person");
        assert_eq!(slugify("!!!"), "migration");
        assert_eq!(slugify("a__b"), "a_b");
        assert_eq!(slugify("_Leading_"), "leading");
    }

    #[test]
    fn new_path_uses_slug_and_tql() {
        let path = new_migration_path(Path::new("/tmp"), "Add Age!");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with("_add_age.tql"));
        assert!(name.chars().take(14).all(|c| c.is_ascii_digit()));
    }
}
