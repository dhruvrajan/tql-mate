//! Unit tests: clap argv parsing (no Docker / TypeDB).

use std::path::PathBuf;

use tqlmate::cli::{Cli, Command};

fn parse(argv: &[&str]) -> Cli {
    Cli::parse_argv(argv).unwrap_or_else(|e| panic!("parse {argv:?}: {e}"))
}

#[test]
fn argv_status_flags() {
    let cli = parse(&["status", "--exit-code", "--quiet"]);
    assert_eq!(
        cli.command,
        Command::Status {
            exit_code: true,
            quiet: true
        }
    );
}

#[test]
fn argv_down_is_rollback() {
    assert_eq!(parse(&["down"]).command, Command::Rollback);
    assert_eq!(parse(&["rollback"]).command, Command::Rollback);
}

#[test]
fn argv_new_with_global_url() {
    let cli = parse(&[
        "-u",
        "typedb://admin:password@localhost/db",
        "new",
        "add_person",
    ]);
    assert_eq!(
        cli.url.as_deref(),
        Some("typedb://admin:password@localhost/db")
    );
    assert_eq!(
        cli.command,
        Command::New {
            name: "add_person".into()
        }
    );
}

#[test]
fn argv_wait_timeout() {
    assert_eq!(
        parse(&["wait", "--timeout", "15"]).command,
        Command::Wait { timeout: 15 }
    );
}

#[test]
fn argv_global_flags_before_subcommand() {
    let cli = parse(&[
        "-v",
        "--strict",
        "-d",
        "migrations",
        "--schema-file",
        "out.tql",
        "migrate",
    ]);
    assert!(cli.verbose);
    assert!(cli.strict);
    assert_eq!(
        cli.migrations_dir.as_deref(),
        Some(PathBuf::from("migrations").as_path())
    );
    assert_eq!(
        cli.schema_file.as_deref(),
        Some(PathBuf::from("out.tql").as_path())
    );
    assert_eq!(cli.command, Command::Migrate);
}

#[test]
fn argv_missing_command_errors() {
    assert!(Cli::parse_argv::<Vec<&str>, &str>(Vec::new()).is_err());
}

#[test]
fn argv_all_subcommands() {
    let cases: &[(&[&str], Command)] = &[
        (&["new", "foo"], Command::New { name: "foo".into() }),
        (&["up"], Command::Up),
        (&["create"], Command::Create),
        (&["drop"], Command::Drop),
        (&["migrate"], Command::Migrate),
        (&["rollback"], Command::Rollback),
        (&["down"], Command::Rollback),
        (
            &["status"],
            Command::Status {
                exit_code: false,
                quiet: false,
            },
        ),
        (
            &["status", "--exit-code", "--quiet"],
            Command::Status {
                exit_code: true,
                quiet: true,
            },
        ),
        (&["dump"], Command::Dump),
        (&["load"], Command::Load),
        (&["wait"], Command::Wait { timeout: 60 }),
        (&["wait", "--timeout", "15"], Command::Wait { timeout: 15 }),
    ];
    for (argv, expect) in cases {
        assert_eq!(parse(argv).command, *expect, "argv={argv:?}");
    }
}

#[test]
fn argv_env_pairs() {
    let cli = parse(&["-e", "FOO=bar", "-e", "BAZ=qux", "status"]);
    assert_eq!(cli.env, vec!["FOO=bar", "BAZ=qux"]);
}
