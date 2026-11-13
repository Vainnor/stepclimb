// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary. The plan's full CLI is `serve | migrate | admin create |
//! export-course | import-course | check-config | backup-verify`; `serve`, `migrate` and
//! `check-config` exist so far, beside `healthcheck` for containers (ADR 0023).
//!
//! Exit codes: 0 on success; 2 when the command line or the configuration is refused, before
//! anything connects; 1 when the database, the migrations or the HTTP server fail, when
//! `check-config` cannot write its report, or when `healthcheck` finds the server not ready.

use std::future::Future;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use stepclimb_server::{Config, healthcheck, startup};
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
    /// Apply database migrations, then exit.
    Migrate,
    /// Validate the configuration without connecting to anything, print its non-secret values,
    /// and exit 0; 2 when it is refused, 1 when the report cannot be written.
    CheckConfig,
    /// Exit 0 when the server on STEPCLIMB_BIND answers 200 from /readyz, else 1. The container
    /// health check, for images without a shell or curl.
    Healthcheck,
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
    match cli.command {
        Command::Serve => {
            init_logging(&config.log);
            run(startup::serve(config))
        }
        Command::Migrate => {
            init_logging(&config.log);
            run(async move { startup::migrate_only(&config).await })
        }
        Command::CheckConfig => check_config(&config),
        Command::Healthcheck => match healthcheck::check(config.bind) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("stepclimb: healthcheck: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

/// Print the configuration's non-secret values to stdout. The configuration was valid to get
/// here.
fn check_config(config: &Config) -> ExitCode {
    let file = config
        .file
        .as_ref()
        .map_or_else(|| "(none)".to_owned(), |path| path.display().to_string());
    let mut report = format!("stepclimb: the configuration is valid\nfile: {file}\n");
    for (key, value) in config.non_secret_values() {
        report.push_str(&format!("{key}: {value}\n"));
    }
    // `write_all` rather than `println!`, which panics on a closed stdout instead of failing.
    match std::io::stdout().write_all(report.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stepclimb: could not write to stdout: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Run `work` on a multi-threaded runtime; any error is logged and printed, and exits 1.
fn run(work: impl Future<Output = Result<(), startup::StartupError>>) -> ExitCode {
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
    match runtime.block_on(work) {
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
