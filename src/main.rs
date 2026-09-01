//! Binary entrypoint. Clap definitions live in `tqlmate::cli` so `tests/cli.rs` can exercise argv.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tqlmate::cli::{Cli, Command};
use tqlmate::{default_migrations_dir, default_schema_file, resolve_url, Opts, Runner};

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

    // `new` only writes a file, so it must not demand a usable connection URL.
    let url = match cli.command {
        Command::New { .. } => None,
        _ => Some(resolve_url(cli.url.as_deref()).context("invalid TypeDB URL")?),
    };

    let opts = Opts {
        url,
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
            // SAFETY: single-threaded CLI startup before worker threads share env.
            unsafe { std::env::set_var(k, v) };
        }
    }
}
