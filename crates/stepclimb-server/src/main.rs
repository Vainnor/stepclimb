// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary. The plan's full CLI is `serve | migrate | admin create |
//! export-course | import-course | check-config | backup-verify`; `serve`, `migrate`,
//! `admin create` and `check-config` exist so far, beside `healthcheck` for containers (ADR
//! 0023) and `admin enrollment-link`, the way back in for an admin without a working link
//! (spec 04 B10).
//!
//! Exit codes: 0 on success; 2 when the command line or the configuration is refused, before
//! anything connects, or when an `admin` command refuses an argument's value or what it finds
//! (an organization already there, an unknown user, a user who can sign in already); 1 when the
//! database, the migrations or the HTTP server fail, when `check-config` or an `admin` command
//! cannot write its report, or when `healthcheck` finds the server not ready.

use std::future::Future;
use std::io::{IsTerminal as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use stepclimb_server::admin::{self, NewOrganizationAdmin};
use stepclimb_server::recovery::{self, RecoveryRequest, UserRef};
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
    /// Administer the instance from the host.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Apply database migrations and write the built-in roles, then create the instance's
    /// organization with its first Organization admin in one transaction. Exit 0 on success; 2
    /// when a value is refused or an organization already exists; 1 when the database fails.
    Create(CreateArgs),
    /// Apply database migrations, then make a new one-time enrollment link for an organization's
    /// admin and print it, replacing the user's earlier unused links. Exit 0 on success; 2 when
    /// the organization or the user is unknown, the user is not an Organization admin or can
    /// already sign in (see the flags); 1 when the database fails.
    EnrollmentLink(EnrollmentLinkArgs),
}

#[derive(Args)]
struct EnrollmentLinkArgs {
    /// The organization's slug.
    #[arg(long, value_name = "SLUG")]
    org: String,
    /// The user's email address (in any letter case) or id.
    #[arg(long, value_name = "EMAIL_OR_ID")]
    user: String,
    /// Make the link for a user who is not an Organization admin at organization scope.
    #[arg(long)]
    admin_required_override: bool,
    /// Make the link for a user who can already sign in.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct CreateArgs {
    /// The organization's slug: 1 to 32 lowercase letters, digits and inner hyphens. It cannot
    /// be changed later.
    #[arg(long, value_name = "SLUG")]
    org_slug: String,
    /// The organization's name.
    #[arg(long, value_name = "NAME")]
    org_name: String,
    /// The first admin's email address.
    #[arg(long, value_name = "EMAIL")]
    email: String,
    /// The first admin's display name.
    #[arg(long, value_name = "NAME")]
    name: String,
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
        Command::Admin {
            command: AdminCommand::Create(args),
        } => {
            init_logging(&config.log);
            admin_create(&config, args)
        }
        Command::Admin {
            command: AdminCommand::EnrollmentLink(args),
        } => {
            init_logging(&config.log);
            admin_enrollment_link(&config, args)
        }
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
    write_stdout(&report)
}

/// `stepclimb admin create`: print what was created to stdout, or the problem to stderr with
/// exit 2 for a refused value and 1 for anything else.
fn admin_create(config: &Config, args: CreateArgs) -> ExitCode {
    let input = NewOrganizationAdmin {
        org_slug: args.org_slug,
        org_name: args.org_name,
        email: args.email,
        name: args.name,
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    match runtime.block_on(admin::create_command(config, &input)) {
        Ok(created) => write_link_report(&admin::report(&created), |error| {
            admin::report_write_failure(&created, error)
        }),
        Err(error) => {
            let (code, message) = admin::failure_report(&error, &input.org_slug);
            if code != 2 {
                tracing::error!(%error, "admin create failed");
            }
            eprint!("{message}");
            ExitCode::from(code)
        }
    }
}

/// `stepclimb admin enrollment-link`: print the link to stdout, or the problem to stderr with
/// exit 2 for a refusal and 1 for anything else.
fn admin_enrollment_link(config: &Config, args: EnrollmentLinkArgs) -> ExitCode {
    let request = RecoveryRequest {
        org_slug: args.org,
        user: UserRef::parse(&args.user),
        admin_required_override: args.admin_required_override,
        force: args.force,
    };
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
    };
    match runtime.block_on(recovery::recovery_command(config, &request)) {
        Ok(recovered) => write_link_report(
            &recovery::report(&recovered),
            recovery::report_write_failure,
        ),
        Err(error) => {
            let (code, message) = recovery::failure_report(&error);
            if code != 2 {
                tracing::error!(%error, "admin enrollment-link failed");
            }
            eprint!("{message}");
            ExitCode::from(code)
        }
    }
}

/// Write a report that ends with an enrollment link to stdout, after warning on stderr when
/// stdout is not a terminal; on a failed write print `failure`'s message and exit 1. The link
/// is already committed by then.
fn write_link_report(text: &str, failure: impl FnOnce(&std::io::Error) -> String) -> ExitCode {
    let mut stdout = std::io::stdout();
    if !stdout.is_terminal() {
        eprint!("{}", admin::LINK_NOT_TERMINAL_WARNING);
    }
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprint!("{}", failure(&error));
            ExitCode::FAILURE
        }
    }
}

/// Write `text` to stdout; exit 1 when it cannot be written.
fn write_stdout(text: &str) -> ExitCode {
    // `write_all` rather than `println!`, which panics on a closed stdout instead of failing.
    match std::io::stdout().write_all(text.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stepclimb: could not write to stdout: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A multi-threaded runtime, or `None` after printing why there is none.
fn runtime() -> Option<tokio::runtime::Runtime> {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => Some(runtime),
        Err(error) => {
            eprintln!("stepclimb: could not start the async runtime: {error}");
            None
        }
    }
}

/// Run `work` on a multi-threaded runtime; any error is logged and printed, and exits 1.
fn run(work: impl Future<Output = Result<(), startup::StartupError>>) -> ExitCode {
    let Some(runtime) = runtime() else {
        return ExitCode::FAILURE;
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
