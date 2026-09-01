//! Clap argv surface shared by the binary and `tests/cli.rs`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug, PartialEq, Eq)]
#[command(
    name = "tqlmate",
    about = "TypeDB 3.x migration tool (dbmate-style)",
    version
)]
pub struct Cli {
    #[arg(short = 'u', long = "url", global = true)]
    pub url: Option<String>,

    #[arg(short = 'e', long = "env", global = true)]
    pub env: Vec<String>,

    #[arg(long = "env-file", global = true, default_value = ".env")]
    pub env_file: PathBuf,

    #[arg(short = 'd', long = "migrations-dir", global = true)]
    pub migrations_dir: Option<PathBuf>,

    #[arg(long = "schema-file", global = true)]
    pub schema_file: Option<PathBuf>,

    #[arg(long = "wait", global = true, value_name = "SECS")]
    pub wait: Option<u64>,

    #[arg(long = "strict", global = true, env = "TQLMATE_STRICT")]
    pub strict: bool,

    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Command {
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

impl Cli {
    pub fn parse_argv<I, S>(argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        Self::try_parse_from(std::iter::once(std::ffi::OsString::from("tqlmate")).chain(
            argv.into_iter().map(|s| {
                let os: std::ffi::OsString = s.into();
                os
            }),
        ))
    }
}
