// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary. The plan's full CLI is `serve | migrate | admin create |
//! export-course | import-course | check-config | backup-verify`; only `serve` exists so far.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use stepclimb_server::{Config, startup};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "stepclimb",
    version,
    about = "Stepclimb learning platform server"
)]
struct Cli {
    /// The configuration file (stepclimb.toml); STEPCLIMB_* variables override it setting by
    /// setting. Without this flag, STEPCLIMB_CONFIG names it; with neither, no file is read.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Apply database migrations, then serve HTTP.
    Serve,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match Config::from_env(cli.config.as_deref()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("stepclimb: configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    init_logging(&config.log);
    match cli.command {
        Command::Serve => serve(config),
    }
}

fn serve(config: Config) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("stepclimb: could not start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(startup::serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "stopped");
            eprintln!("stepclimb: {error}");
            ExitCode::FAILURE
        }
    }
}

/// JSON logs (plan, Technical -> Hosting and operations) to stderr, filtered by the `log`
/// setting, which `Config` has already validated.
fn init_logging(filter: &str) {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .init();
}
