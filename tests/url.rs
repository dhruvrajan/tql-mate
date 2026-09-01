//! Unit tests: TypeDB URL parsing and env-alias resolution (no Docker / TypeDB).

use tqlmate::{resolve_url_from, TypeDbUrl};

#[test]
fn table_driven_parse() {
    let ok_cases = [
        (
            "typedb://admin:secret@db.example:1730/mydb?tls=true",
            TypeDbUrl {
                username: "admin".into(),
                password: "secret".into(),
                host: "db.example".into(),
                port: 1730,
                database: "mydb".into(),
                tls: true,
            },
        ),
        (
            "typedb://admin:password@localhost/app",
            TypeDbUrl {
                username: "admin".into(),
                password: "password".into(),
                host: "localhost".into(),
                port: 1729,
                database: "app".into(),
                tls: false,
            },
        ),
        (
            "typedb://user:p%40ss@localhost/db",
            TypeDbUrl {
                username: "user".into(),
                password: "p@ss".into(),
                host: "localhost".into(),
                port: 1729,
                database: "db".into(),
                tls: false,
            },
        ),
        (
            // Credentials omitted: TypeDB CE defaults fill in.
            "typedb://localhost:1729/mydb",
            TypeDbUrl {
                username: "admin".into(),
                password: "password".into(),
                host: "localhost".into(),
                port: 1729,
                database: "mydb".into(),
                tls: false,
            },
        ),
        (
            "typedb://localhost/mydb?tls=true",
            TypeDbUrl {
                username: "admin".into(),
                password: "password".into(),
                host: "localhost".into(),
                port: 1729,
                database: "mydb".into(),
                tls: true,
            },
        ),
        (
            // A literal '@' in the password: split on the last one.
            "typedb://u:p@ss@host/db",
            TypeDbUrl {
                username: "u".into(),
                password: "p@ss".into(),
                host: "host".into(),
                port: 1729,
                database: "db".into(),
                tls: false,
            },
        ),
        (
            "typedb://u:p+word@127.0.0.1:1729/x?tls=1",
            TypeDbUrl {
                username: "u".into(),
                password: "p word".into(),
                host: "127.0.0.1".into(),
                port: 1729,
                database: "x".into(),
                tls: true,
            },
        ),
        (
            "typedb://u:p@host/db?tls=yes",
            TypeDbUrl {
                username: "u".into(),
                password: "p".into(),
                host: "host".into(),
                port: 1729,
                database: "db".into(),
                tls: true,
            },
        ),
        (
            "typedb://u:p@host/db?tls=false",
            TypeDbUrl {
                username: "u".into(),
                password: "p".into(),
                host: "host".into(),
                port: 1729,
                database: "db".into(),
                tls: false,
            },
        ),
        (
            "typedb://u:p@host/nested/path",
            TypeDbUrl {
                username: "u".into(),
                password: "p".into(),
                host: "host".into(),
                port: 1729,
                database: "nested/path".into(),
                tls: false,
            },
        ),
    ];
    for (raw, expect) in ok_cases {
        assert_eq!(TypeDbUrl::parse(raw).unwrap(), expect, "parse({raw:?})");
    }

    let err_cases = [
        ("http://admin:x@localhost/db", "UrlScheme"),
        ("typedb://admin@localhost/db", "UrlAuthPair"),
        ("typedb://admin:pass@localhost", "UrlDatabase"),
        ("typedb://localhost:1729", "UrlDatabase"),
        ("typedb://admin:pass@localhost/", "UrlDatabase"),
        ("typedb://admin:pass@localhost:xyz/db", "UrlPort"),
    ];
    for (raw, kind) in err_cases {
        let err = TypeDbUrl::parse(raw).expect_err(raw);
        let label = format!("{err:?}");
        assert!(label.contains(kind), "parse({raw:?}) => {err:?}");
    }
}

#[test]
fn address_uses_host_port() {
    let u = TypeDbUrl::parse("typedb://a:b@db.example:1730/mydb").unwrap();
    assert_eq!(u.address(), "db.example:1730");
}

#[test]
fn with_database_preserves_other_fields() {
    let u = TypeDbUrl::parse("typedb://a:b@h:1/old?tls=true").unwrap();
    let n = u.with_database("new_db");
    assert_eq!(n.database, "new_db");
    assert_eq!(n.host, "h");
    assert!(n.tls);
}

#[test]
fn resolve_url_from_precedence() {
    let cases = [
        (
            Some("typedb://a:b@h:1/cli"),
            Some("typedb://a:b@h:1/typedb_env"),
            Some("typedb://a:b@h:1/database_env"),
            "cli",
        ),
        (
            None,
            Some("typedb://a:b@h:1/typedb_env"),
            Some("typedb://a:b@h:1/database_env"),
            "typedb_env",
        ),
        (
            None,
            None,
            Some("typedb://a:b@h:1/database_env"),
            "database_env",
        ),
        (None, None, None, "typedb"),
    ];
    for (cli, typedb, database, expect_db) in cases {
        let u = resolve_url_from(cli, typedb, database).unwrap();
        assert_eq!(
            u.database, expect_db,
            "cli={cli:?} typedb={typedb:?} database={database:?}"
        );
    }
}
