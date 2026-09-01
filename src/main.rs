use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tqlmate::{default_migrations_dir, default_schema_file, resolve_url, Opts, Runner};

#[derive(Parser, Debug)]
#[command(
    name = "tqlmate",
    about = "TypeDB 3.x migration tool (dbmate-style)",
    version
)]
struct Cli {
    #[arg(short = 'u', long = "url", global = true)]
    url: Option<String>,

    #[arg(short = 'e', long = "env", global = true)]
    env: Vec<String>,

    #[arg(long = "env-file", global = true, default_value = ".env")]
    env_file: PathBuf,

    #[arg(short = 'd', long = "migrations-dir", global = true)]
    migrations_dir: Option<PathBuf>,

    #[arg(long = "schema-file", global = true)]
    schema_file: Option<PathBuf>,

    #[arg(long = "wait", global = true, value_name = "SECS")]
    wait: Option<u64>,

    #[arg(long = "strict", global = true, env = "TQLMATE_STRICT")]
    strict: bool,

    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
enum Command {
    New {
        name: String,
    },
    Up,
    Create,
    Drop,
    Migrate,
    #[command(visible_alias = "down")]
    Rollback,
    Status {
        #[arg(long = "exit-code")]
        exit_code: bool,
        #[arg(long = "quiet")]
        quiet: bool,
    },
    Dump,
    Load,
    Wait {
        #[arg(long = "timeout", default_value = "60")]
        timeout: u64,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    load_env(&cli);

    let opts = Opts {
        url: resolve_url(cli.url.as_deref()).context("invalid TypeDB URL")?,
        migrations_dir: cli.migrations_dir.unwrap_or_else(default_migrations_dir),
        schema_file: cli.schema_file.unwrap_or_else(default_schema_file),
        strict: cli.strict,
        verbose: cli.verbose,
        wait_timeout: cli.wait.map(Duration::from_secs),
    };
    let mut runner = Runner::new(opts);

    match cli.command {
        Command::New { name } => {
            runner
                .new_migration(&name)
                .await
                .context("failed to create migration")?;
        }
        Command::Up => runner.up().await.context("up failed")?,
        Command::Create => runner.create().await.context("create failed")?,
        Command::Drop => runner.drop().await.context("drop failed")?,
        Command::Migrate => runner.migrate().await.context("migrate failed")?,
        Command::Rollback => runner.rollback().await.context("rollback failed")?,
        Command::Status { exit_code, quiet } => {
            let pending = runner.status(quiet).await.context("status failed")?;
            if exit_code && pending {
                return Ok(ExitCode::from(1));
            }
        }
        Command::Dump => runner.dump().await.context("dump failed")?,
        Command::Load => runner.load().await.context("load failed")?,
        Command::Wait { timeout } => {
            let secs = cli.wait.unwrap_or(timeout);
            runner
                .wait(Duration::from_secs(secs))
                .await
                .context("wait failed")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn load_env(cli: &Cli) {
    let _ = dotenvy::from_path(&cli.env_file);
    for pair in &cli.env {
        if let Some((k, v)) = pair.split_once('=') {
            // SAFETY: single-threaded CLI startup before any worker threads share env.
            unsafe { std::env::set_var(k, v) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(argv: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("tqlmate").chain(argv.iter().copied()))
            .unwrap_or_else(|e| panic!("parse {argv:?}: {e}"))
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
        let cli = parse(&["down"]);
        assert_eq!(cli.command, Command::Rollback);
        let cli = parse(&["rollback"]);
        assert_eq!(cli.command, Command::Rollback);
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
        let cli = parse(&["wait", "--timeout", "15"]);
        assert_eq!(cli.command, Command::Wait { timeout: 15 });
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
        assert!(Cli::try_parse_from(["tqlmate"]).is_err());
    }

    #[test]
    fn argv_env_pairs() {
        let cli = parse(&["-e", "FOO=bar", "-e", "BAZ=qux", "status"]);
        assert_eq!(cli.env, vec!["FOO=bar", "BAZ=qux"]);
    }
}
