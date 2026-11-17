// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb serve --role` (docs/specs/m1/01-job-queue-and-outbox.md B13 to B16, T11, T12): the
//! binary is started against a fresh database per test, its JSON log on stderr is read line by
//! line, it is asked over HTTP what it serves, and it is stopped with SIGTERM. The
//! `probes_router` cases go through the router without a socket. Needs `DATABASE_URL`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use axum::{body::Body, http::Request, http::StatusCode};
use serde_json::Value;
use sqlx::{ConnectOptions, PgPool};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::clock::ManualClock;
use stepclimb_server::{AppState, app::probes_router, startup, web_origin::WebOrigin};
use tower::ServiceExt;

const KEYS: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// How long a test waits for the server to log a line or exit.
const PATIENCE: Duration = Duration::from_secs(60);

/// A running `stepclimb serve`, killed on drop if a test fails before stopping it.
struct Server {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start `stepclimb serve` with `args` on a free port of 127.0.0.1, against `pool`'s database.
fn serve(pool: &PgPool, args: &[&str], env: &[(&str, &str)]) -> Server {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stepclimb"));
    for (key, _) in std::env::vars() {
        if key.starts_with("STEPCLIMB_") {
            command.env_remove(key);
        }
    }
    command
        .arg("serve")
        .args(args)
        .env(
            "STEPCLIMB_DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_PUBLIC_URL", "http://localhost:8080")
        .env("STEPCLIMB_BIND", "127.0.0.1:0")
        .env("STEPCLIMB_LOG", "info")
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the binary starts");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Server {
        child,
        lines,
        seen: Vec::new(),
    }
}

impl Server {
    /// The first log line from now on whose message is `message`, as JSON.
    fn wait_for(&mut self, message: &str) -> Value {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).unwrap_or_else(|_| {
                panic!("no {message:?} log line; seen:\n{}", self.seen.join("\n"))
            });
            self.seen.push(line.clone());
            let Ok(json) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if json["fields"]["message"] == message {
                return json;
            }
        }
    }

    /// The address the server listens on, from its `listening` line.
    fn listening(&mut self) -> String {
        let line = self.wait_for("listening");
        line["fields"]["addr"]
            .as_str()
            .expect("the listening line names its address")
            .to_owned()
    }

    /// Send SIGTERM and wait for the exit, then collect the rest of the log.
    fn terminate(&mut self) -> ExitStatus {
        let pid = self.child.id().to_string();
        let sent = Command::new("sh")
            .args(["-c", &format!("kill -TERM {pid}")])
            .status()
            .expect("sh runs");
        assert!(sent.success(), "kill -TERM {pid}");
        let deadline = Instant::now() + PATIENCE;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("wait on the server") {
                break status;
            }
            assert!(Instant::now() < deadline, "the server did not exit");
            std::thread::sleep(Duration::from_millis(20));
        };
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(2)) {
            self.seen.push(line);
        }
        status
    }

    fn logged(&self, message: &str) -> bool {
        self.seen
            .iter()
            .any(|line| line.contains(&format!(r#""message":"{message}""#)))
    }
}

/// The connections to `pool`'s database named [`startup::JOBS_APPLICATION_NAME`], the job
/// worker's pool. The worker connects it before the listener binds.
async fn job_connections(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = current_database() AND application_name = $1",
    )
    .bind(startup::JOBS_APPLICATION_NAME)
    .fetch_one(pool)
    .await
    .expect("read pg_stat_activity")
}

/// Wait until every periodic job has been scheduled past its first run and has run: each
/// schedule row has moved on from the time startup wrote it, and no job is left.
async fn periodic_jobs_ran(pool: &PgPool) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let moved: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM job_schedules WHERE next_run_at > created_at ORDER BY name",
        )
        .fetch_all(pool)
        .await
        .expect("read schedules");
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
            .fetch_one(pool)
            .await
            .expect("count jobs");
        let all = [
            "auth.flows_cleanup",
            "files.cleanup",
            "jobs.prune",
            "videos.cleanup",
        ];
        if moved == all && left == 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "schedules moved: {moved:?}, jobs left: {left}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The status code of a plain HTTP/1.1 GET of `path` from `addr`.
fn status_of(addr: &str, path: &str) -> u16 {
    let mut stream = TcpStream::connect(addr).expect("connect to the server");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nhost: localhost:8080\r\nconnection: close\r\n\r\n"
    )
    .expect("send the request");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let status = response
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("no status line in {response:?}"));
    status.parse().expect("a numeric status")
}

// T11 (B13): `worker` runs the job worker and answers only the probes; `--role` wins over the
// variable. T12 (B15): SIGTERM stops it cleanly.
#[sqlx::test(migrations = false)]
async fn a_worker_process_runs_jobs_and_answers_only_the_probes(pool: PgPool) {
    let mut server = serve(&pool, &["--role", "worker"], &[("STEPCLIMB_ROLE", "web")]);
    let started = server.wait_for("job worker started");
    assert_eq!(started["fields"]["concurrency"], "4", "{started}");
    let addr = server.listening();
    assert_eq!(status_of(&addr, "/healthz"), 200);
    assert_eq!(status_of(&addr, "/readyz"), 200);
    assert_eq!(status_of(&addr, "/api/v1/me"), 404);
    assert_eq!(status_of(&addr, "/"), 404, "no SPA either");
    assert!(
        job_connections(&pool).await > 0,
        "the worker has its own pool"
    );
    // Spec m1/01 B17, B18: startup wrote the schedules, the first tick enqueued every periodic
    // job, and the worker ran them.
    periodic_jobs_ran(&pool).await;
    let status = server.terminate();
    assert!(
        status.success(),
        "{status}; log:\n{}",
        server.seen.join("\n")
    );
    assert!(
        server.logged("job worker stopping"),
        "{}",
        server.seen.join("\n")
    );
    assert!(
        server.logged("job worker stopped"),
        "{}",
        server.seen.join("\n")
    );
}

// T11 (B13): `web` serves the application and runs no job worker.
#[sqlx::test(migrations = false)]
async fn a_web_process_serves_the_application_and_runs_no_worker(pool: PgPool) {
    let mut server = serve(&pool, &["--role", "web"], &[]);
    let addr = server.listening();
    assert_eq!(status_of(&addr, "/healthz"), 200);
    assert_eq!(status_of(&addr, "/api/v1/me"), 401, "the API is served");
    assert_eq!(job_connections(&pool).await, 0, "no job pool");
    let schedules: i64 = sqlx::query_scalar("SELECT count(*) FROM job_schedules")
        .fetch_one(&pool)
        .await
        .expect("count schedules");
    assert_eq!(schedules, 0, "a web process writes no schedules");
    let status = server.terminate();
    assert!(
        status.success(),
        "{status}; log:\n{}",
        server.seen.join("\n")
    );
    assert!(
        !server.seen.iter().any(|line| line.contains("job worker")),
        "{}",
        server.seen.join("\n")
    );
}

// B13: `all`, the default, serves the application and runs the job worker.
#[sqlx::test(migrations = false)]
async fn an_all_process_serves_the_application_and_runs_the_worker(pool: PgPool) {
    let mut server = serve(&pool, &[], &[("STEPCLIMB_WORKER_CONCURRENCY", "7")]);
    let started = server.wait_for("job worker started");
    assert_eq!(started["fields"]["concurrency"], "7", "{started}");
    let addr = server.listening();
    assert_eq!(status_of(&addr, "/api/v1/me"), 401, "the API is served");
    let status = server.terminate();
    assert!(
        status.success(),
        "{status}; log:\n{}",
        server.seen.join("\n")
    );
    assert!(
        server.logged("job worker stopped"),
        "{}",
        server.seen.join("\n")
    );
}

#[test]
fn an_unknown_role_flag_exits_2_naming_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_stepclimb"))
        .args(["serve", "--role", "other"])
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("'--role <ROLE>'"), "{stderr}");
    assert!(
        stderr.contains("[possible values: all, web, worker]"),
        "{stderr}"
    );
}

/// Application state over `db`, with a stopped clock and a test-only key.
fn state(db: PgPool) -> AppState {
    AppState {
        db,
        clock: ManualClock::new(std::time::SystemTime::UNIX_EPOCH).clock(),
        keys: KeyRing::parse(KEYS).expect("a valid test key"),
        public_url: WebOrigin::parse("http://localhost:8080").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        passwords: Default::default(),
        storage: None,
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

async fn probe_status(pool: &PgPool, path: &str) -> StatusCode {
    probes_router(state(pool.clone()))
        .oneshot(Request::get(path).body(Body::empty()).expect("a request"))
        .await
        .expect("the router never fails")
        .status()
}

// B13: a wrong method on a probe answers the problem-details 405 the whole application does,
// and an https public URL adds HSTS to a worker's answers too.
#[sqlx::test(migrations = false)]
async fn the_probes_router_answers_405_problems_and_sends_hsts(pool: PgPool) {
    let response = probes_router(state(pool.clone()))
        .oneshot(
            Request::post("/healthz")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router never fails");
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert!(
        response
            .headers()
            .get("strict-transport-security")
            .is_none()
    );

    let mut https = state(pool);
    https.public_url = WebOrigin::parse("https://lms.example.org").expect("a valid origin");
    let response = probes_router(https)
        .oneshot(
            Request::get("/healthz")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router never fails");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["strict-transport-security"],
        stepclimb_server::app::HSTS
    );
}

// B13: a worker's router has the two probes and nothing else.
#[sqlx::test(migrations = false)]
async fn the_probes_router_answers_the_probes_and_404_for_the_rest(pool: PgPool) {
    assert_eq!(probe_status(&pool, "/healthz").await, StatusCode::OK);
    assert_eq!(probe_status(&pool, "/readyz").await, StatusCode::OK);
    for path in [
        "/api/v1/me",
        "/api/openapi.json",
        "/docs",
        "/",
        "/auth/sign-out",
    ] {
        assert_eq!(
            probe_status(&pool, path).await,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}
