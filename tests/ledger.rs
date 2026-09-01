//! Unit tests: ledger / dump TypeQL strings (no Docker / TypeDB).

use std::path::PathBuf;

use tqlmate::{
    bootstrap_schema, dump_header, record_delete, record_insert, strip_dump_header, ATTR_APPLIED_AT,
    ATTR_VERSION, ENTITY, MigrationFile, Version,
};

#[test]
fn ledger_typeql_insert_delete() {
    let v = Version::new("20240101120000");
    assert_eq!(
        record_insert(&v, "2024-01-01T12:00:00"),
        format!(
            "insert $_ isa {ENTITY}, has {ATTR_VERSION} \"20240101120000\", has {ATTR_APPLIED_AT} 2024-01-01T12:00:00;"
        )
    );
    assert_eq!(
        record_delete(&v),
        format!("match $m isa {ENTITY}, has {ATTR_VERSION} \"20240101120000\"; delete $m;")
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
    let text =
        "-- Schema dumped by tqlmate\n-- Applied migrations:\n--   1_person\n\ndefine\n  entity x;\n";
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
