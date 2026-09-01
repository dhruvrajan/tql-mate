//! Unit tests: migration filenames, section parse, order, slugify (no Docker / TypeDB).

use std::io::Write;
use std::path::Path;

use tqlmate::{
    check_strict_order, list_migration_files, new_migration_path, parse_migration,
    parse_migration_body, parse_version_name, slugify, status_rows, Error, MigrationFile,
    MigrationStatus, Version,
};

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
        let (u, d) = parse_migration_body(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        assert_eq!((u.as_str(), d.as_str()), (*eu, *ed), "text={text:?}");
    }

    for text in ["no markers here", ""] {
        assert!(
            matches!(parse_migration_body(text), Err(Error::MigrationMarkers)),
            "{text:?}"
        );
    }
}

#[test]
fn parse_migration_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("20240101120000_demo.tql");
    let mut f = std::fs::File::create(&path).unwrap();
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
    assert!(check_strict_order(&files, &[Version::new("1")]).is_ok());
}

#[test]
fn lists_sorted_and_rejects_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["20240201000000_b.tql", "20240101000000_a.tql"] {
        let mut f = std::fs::File::create(dir.path().join(name)).unwrap();
        write!(f, "-- migrate:up\n\n-- migrate:down\n").unwrap();
    }
    let files = list_migration_files(dir.path()).unwrap();
    assert_eq!(files[0].version.as_str(), "20240101000000");
    assert_eq!(files[1].version.as_str(), "20240201000000");

    let mut f = std::fs::File::create(dir.path().join("20240101000000_dup.tql")).unwrap();
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
