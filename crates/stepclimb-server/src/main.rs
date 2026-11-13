// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary. The plan's full CLI is `serve | migrate | admin create |
//! export-course | import-course | check-config | backup-verify`; only `serve` exists so far.

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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Apply database migrations, then serve HTTP. Configured by STEPCLIMB_* variables.
    Serve,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging();
    match cli.command {
        Command::Serve => serve(),
    }
}

fn serve() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("stepclimb: configuration error: {error}");
            return ExitCode::from(2);
        }
    };
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

/// JSON logs (plan, Technical -> Hosting and operations), filtered by `STEPCLIMB_LOG`
/// (`info` when unset), to stderr.
fn init_logging() {
    let filter =
        EnvFilter::try_from_env("STEPCLIMB_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
