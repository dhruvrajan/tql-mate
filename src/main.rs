use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tqlmate::{Opts, Runner, default_migrations_dir, default_schema_file, resolve_url};

#[derive(Parser, Debug)]
#[command(name = "tqlmate", about = "TypeDB 3.x migration tool (dbmate-style)", version)]
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

#[derive(Subcommand, Debug)]
enum Command {
    New { name: String },
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
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> tqlmate::Result<ExitCode> {
    let cli = Cli::parse();
    load_env(&cli);

    let opts = Opts {
        url: resolve_url(cli.url.as_deref())?,
        migrations_dir: cli.migrations_dir.unwrap_or_else(default_migrations_dir),
        schema_file: cli.schema_file.unwrap_or_else(default_schema_file),
        strict: cli.strict,
        verbose: cli.verbose,
        wait_timeout: cli.wait.map(Duration::from_secs),
    };
    let mut runner = Runner::new(opts);

    match cli.command {
        Command::New { name } => {
            runner.new_migration(&name).await?;
        }
        Command::Up => runner.up().await?,
        Command::Create => runner.create().await?,
        Command::Drop => runner.drop().await?,
        Command::Migrate => runner.migrate().await?,
        Command::Rollback => runner.rollback().await?,
        Command::Status { exit_code, quiet } => {
            let pending = runner.status(quiet).await?;
            if exit_code && pending {
                return Ok(ExitCode::from(1));
            }
        }
        Command::Dump => runner.dump().await?,
        Command::Load => runner.load().await?,
        Command::Wait { timeout } => {
            let secs = cli.wait.unwrap_or(timeout);
            runner.wait(Duration::from_secs(secs)).await?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn load_env(cli: &Cli) {
    let _ = dotenvy::from_path(&cli.env_file);
    for pair in &cli.env {
        if let Some((k, v)) = pair.split_once('=') {
            std::env::set_var(k, v);
        }
    }
}
