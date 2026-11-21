// SPDX-License-Identifier: AGPL-3.0-or-later
//! `video.transcode` end to end against the dev shell's ffmpeg, a fresh database per test and a
//! temporary local store (docs/specs/m1/08-uploaded-video.md B8 to B13, B21; T4, T6 to T8).
//!
//! Each test makes a `processing` video the way a completed upload leaves it (its parts stored
//! under `upload/<offset>`, two parts so the job concatenates), enqueues its job as the
//! completing chunk does, and runs one pass of a worker of the `transcode` queue with the
//! registry `stepclimb serve` builds. Every clip is generated here from ffmpeg's `testsrc` and
//! `sine` sources. Needs `DATABASE_URL`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use object_store::{ObjectStoreExt as _, PutPayload};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    clock::ManualClock,
    courses,
    id::{Uuid, new_id},
    organizations,
    transcodes::{self, Retried},
    users::{self, NewUser},
    videos::{self, NewUpload, Terminating},
};
use stepclimb_jobs::{NewJob, Worker, backoff::Random, enqueue, queue};
use stepclimb_media::tools::{Tools, find_on_path};
use stepclimb_server::{
    config::StorageConfig,
    jobs,
    storage::Storage,
    video_parts,
    video_transcode::{LEASE, LEASE_EVERY, TRANSCODE, Transcoder},
};

/// 2026-03-31T23:00:00Z: an hour before a month ends.
const MARCH_END: u64 = 1_774_998_000;

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the routes and jobs that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("stepclimb-{name}-{}", new_id().simple()));
    std::fs::create_dir_all(&dir).expect("create the directory");
    TempDir(dir)
}

fn tools(ffmpeg: Option<PathBuf>) -> Tools {
    Tools {
        ffmpeg: ffmpeg.unwrap_or_else(|| find_on_path("ffmpeg").expect("ffmpeg on PATH")),
        ffprobe: find_on_path("ffprobe").expect("ffprobe on PATH"),
    }
}

/// A clip from ffmpeg's test sources: `video` (`width`, `height`) or none, `sound` or not,
/// `seconds` long, in the container `extension` names.
fn clip(
    dir: &Path,
    extension: &str,
    video: Option<(u32, u32)>,
    sound: bool,
    seconds: u32,
) -> Vec<u8> {
    let out = dir.join(format!("{}.{extension}", new_id().simple()));
    let mut command = Command::new(find_on_path("ffmpeg").expect("ffmpeg on PATH"));
    command.args(["-nostdin", "-loglevel", "error", "-y"]);
    if let Some((w, h)) = video {
        command.args(["-f", "lavfi", "-i"]);
        command.arg(format!("testsrc=size={w}x{h}:rate=25:duration={seconds}"));
    }
    if sound {
        command.args(["-f", "lavfi", "-i"]);
        command.arg(format!("sine=frequency=440:duration={seconds}"));
    }
    if video.is_some() {
        command.args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ]);
    }
    if sound {
        command.args(["-c:a", "aac"]);
    }
    let status = command.arg(&out).status().expect("ffmpeg runs");
    assert!(status.success(), "the fixture was made");
    std::fs::read(out).expect("read the fixture")
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    storage: Storage,
    org: Uuid,
    course: Uuid,
    user: Uuid,
    work: TempDir,
    _store: TempDir,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let store = temp_dir("transcode-store");
        let storage = Storage::open(
            &StorageConfig::Local {
                root: store.0.clone(),
            },
            64 * 1024,
        )
        .expect("open the store")
        .expect("storage is configured");
        let clock = ManualClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(MARCH_END));
        let now = clock.clock().now();
        let slug = format!("org-{}", &new_id().simple().to_string()[24..]);
        let org = organizations::create(&pool, &slug, &slug, now)
            .await
            .expect("create organization")
            .id;
        let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        let new = courses::NewCourse {
            account_id: root,
            title: "Course V1".into(),
            code: "V1".into(),
        };
        let course = courses::create(&pool, scope(org), &new, now, Actor::Instance)
            .await
            .expect("create course")
            .id;
        let new_user = NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(&pool, scope(org), &new_user, now)
            .await
            .expect("create user")
            .id;
        Self {
            pool,
            clock,
            storage,
            org,
            course,
            user,
            work: temp_dir("transcode-work"),
            _store: store,
        }
    }

    fn now(&self) -> SystemTime {
        self.clock.clock().now()
    }

    fn transcoder(&self, max_video_seconds: u32, ffmpeg: Option<PathBuf>) -> Transcoder {
        Transcoder {
            tools: tools(ffmpeg),
            threads: 2,
            dir: self.work.0.clone(),
            max_video_seconds,
        }
    }

    /// A `processing` video whose upload holds `bytes` in two parts, its transcode job
    /// enqueued: its id.
    async fn processing(&self, bytes: &[u8]) -> Uuid {
        let s = scope(self.org);
        let new = NewUpload {
            course_id: self.course,
            title: "-i http://example.com $(touch x)",
            length: bytes.len() as u64,
            created_by: self.user,
        };
        let mut tx = self.pool.begin().await.expect("a fixture step");
        let (video, upload) = videos::create_upload(&mut tx, s, &new, self.now())
            .await
            .expect("a fixture step");
        let half = bytes.len() / 2;
        for (offset, part) in [(0, &bytes[..half]), (half, &bytes[half..])] {
            let path = self.storage.video_part_path(s, video.id, offset as u64);
            self.storage
                .store()
                .put(&path, PutPayload::from(part.to_vec()))
                .await
                .expect("a fixture step");
            videos::advance(
                &mut tx,
                s,
                upload.id,
                (offset as u64, part.len() as u64),
                self.now(),
            )
            .await
            .expect("a fixture step");
        }
        videos::finish_upload(&mut tx, s, video.id, None, self.now())
            .await
            .expect("a fixture step");
        self.enqueue(&mut tx, video.id).await;
        tx.commit().await.expect("a fixture step");
        video.id
    }

    async fn enqueue(&self, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, video: Uuid) {
        let payload = serde_json::json!({ "video_id": video });
        let key = video.to_string();
        let job = NewJob::org(&TRANSCODE, scope(self.org), &payload)
            .expect("a fixture step")
            .dedupe(&key)
            .expect("a fixture step");
        enqueue(tx, job, self.now()).await.expect("a fixture step");
    }

    /// One pass of a `transcode` worker with `transcoder`: how many jobs it ran.
    async fn work(&self, transcoder: Transcoder) -> usize {
        self.work_with(Some(self.storage.clone()), Some(transcoder))
            .await
    }

    /// One pass of a `transcode` worker of a process with `storage` and `transcoder`.
    async fn work_with(&self, storage: Option<Storage>, transcoder: Option<Transcoder>) -> usize {
        let registry = jobs::registry(storage, transcoder).expect("a fixture step");
        let worker = Worker::new(
            self.pool.clone(),
            registry,
            self.clock.clock(),
            Random::system(),
            vec![jobs::TRANSCODE_QUEUE],
        );
        worker.run_once(4).await.expect("a pass")
    }

    async fn status(&self, video: Uuid) -> (String, Option<String>) {
        sqlx::query_as("SELECT status, error_code FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&self.pool)
            .await
            .expect("a fixture step")
    }

    /// Every object key of the organization, relative to the store, sorted.
    async fn objects(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let mut pending = vec![self.storage.path(&format!("orgs/{}", self.org))];
        while let Some(at) = pending.pop() {
            let page = self
                .storage
                .store()
                .list_with_delimiter(Some(&at))
                .await
                .expect("a fixture step");
            keys.extend(
                page.objects
                    .iter()
                    .filter_map(|o| self.storage.key_of(&o.location)),
            );
            pending.extend(page.common_prefixes);
        }
        keys.sort();
        keys
    }

    async fn uploads(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM video_uploads WHERE org_id = $1")
            .bind(self.org)
            .fetch_one(&self.pool)
            .await
            .expect("a fixture step")
    }

    async fn usage(&self) -> Vec<(String, i32)> {
        sqlx::query_as(
            "SELECT month::text, minutes FROM transcode_usage WHERE org_id = $1 ORDER BY month",
        )
        .bind(self.org)
        .fetch_all(&self.pool)
        .await
        .expect("a fixture step")
    }

    /// Put the organization on a test plan with `limits`, by SQL (M1 has no API for it).
    async fn limit(&self, limits: serde_json::Value) {
        let plan = new_id();
        sqlx::query("INSERT INTO plans (id, key, limits) VALUES ($1, $2, $3::text::jsonb)")
            .bind(plan)
            .bind(format!("test_{}", plan.simple()))
            .bind(limits.to_string())
            .execute(&self.pool)
            .await
            .expect("a fixture step");
        sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
            .bind(self.org)
            .bind(plan)
            .execute(&self.pool)
            .await
            .expect("a fixture step");
    }

    /// What a run that charged `minutes` and then died leaves: its run taken and its charge
    /// recorded on the row, with nothing ended.
    async fn charge_without_ending(&self, video: Uuid, minutes: u64) {
        let s = scope(self.org);
        let mut tx = self.pool.begin().await.expect("a fixture step");
        let started = transcodes::start(&mut tx, s, video, self.now())
            .await
            .expect("a fixture step");
        let transcodes::Start::Run { run, .. } = started else {
            panic!("the video is processing with its original: {started:?}")
        };
        let charged = transcodes::charge(&mut tx, s, (video, run), minutes, self.now())
            .await
            .expect("a fixture step");
        assert!(charged);
        tx.commit().await.expect("a fixture step");
    }

    /// The video's open charge, `charged_minutes`, if any.
    async fn charge_record(&self, video: Uuid) -> Option<i32> {
        sqlx::query_scalar("SELECT charged_minutes FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&self.pool)
            .await
            .expect("a fixture step")
    }

    async fn retries(&self, video: Uuid) -> i32 {
        sqlx::query_scalar("SELECT transcode_retries FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&self.pool)
            .await
            .expect("a fixture step")
    }

    async fn retry(&self, video: Uuid) -> Retried {
        let mut tx = self.pool.begin().await.expect("a fixture step");
        let retried = transcodes::retry(
            &mut tx,
            scope(self.org),
            (video, &[self.course]),
            self.now(),
        )
        .await
        .expect("a fixture step");
        if retried == Retried::Processing {
            self.enqueue(&mut tx, video).await;
        }
        tx.commit().await.expect("a fixture step");
        retried
    }

    fn prefix(&self, video: Uuid) -> String {
        format!("orgs/{}/videos/{video}", self.org)
    }
}

/// T6, T7, B12, B13: a 720p clip becomes `ready` with 360p and 720p, its outputs under `v1/`
/// and its parts and upload row gone, its minute recorded for the month; a second run of the
/// job changes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_processing_video_becomes_ready_and_a_second_run_changes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let bytes = clip(&f.work.0, "mp4", Some((1280, 720)), true, 3);
    let video = f.processing(&bytes).await;
    assert_eq!(f.work(f.transcoder(60, None)).await, 1);
    assert_eq!(f.status(video).await, ("ready".to_owned(), None));
    let mut conn = f.pool.acquire().await.unwrap();
    let read = videos::get(&mut conn, scope(f.org), video)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read.heights, [360, 720]);
    assert!((read.duration_s.unwrap() - 3.0).abs() < 0.1);
    let prefix = f.prefix(video);
    let objects = f.objects().await;
    let expected = [
        "v1/360p/index.m3u8",
        "v1/360p/segment_00000.ts",
        "v1/720p/index.m3u8",
        "v1/720p/segment_00000.ts",
        "v1/master.m3u8",
        "v1/poster.jpg",
        "v1/sprite_000.jpg",
        "v1/thumbnails.vtt",
        "v1/video.mp4",
    ]
    .map(|name| format!("{prefix}/{name}"));
    assert_eq!(objects, expected, "the parts are gone (Q3)");
    assert_eq!(f.uploads().await, 0, "the upload row went with its parts");
    let (stored, poster, renditions): (i64, String, serde_json::Value) =
        sqlx::query_as("SELECT storage_bytes, poster_key, renditions FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let mut sum = 0i64;
    for key in &objects {
        let meta = f.storage.store().head(&f.storage.path(key)).await.unwrap();
        sum += meta.size as i64;
    }
    assert_eq!(stored, sum);
    assert_eq!(poster, format!("{prefix}/v1/poster.jpg"));
    assert_eq!(renditions["master"], format!("{prefix}/v1/master.m3u8"));
    assert_eq!(renditions["mp4"], format!("{prefix}/v1/video.mp4"));
    assert_eq!(renditions["sprites"], format!("{prefix}/v1/thumbnails.vtt"));
    assert_eq!(
        renditions["bitrates"],
        serde_json::json!([800_000, 2_800_000])
    );
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 1)]);

    let updated: SystemTime = updated_at(&f.pool, video).await;
    let mut tx = f.pool.begin().await.unwrap();
    f.enqueue(&mut tx, video).await;
    tx.commit().await.unwrap();
    assert_eq!(f.work(f.transcoder(60, None)).await, 1);
    assert_eq!(
        updated_at(&f.pool, video).await,
        updated,
        "B12: run twice, nothing changes"
    );
    assert_eq!(f.objects().await, expected);
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 1)]);
}

async fn updated_at(pool: &PgPool, video: Uuid) -> SystemTime {
    let us: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM updated_at) * 1000000)::bigint FROM videos WHERE id = $1",
    )
    .bind(video)
    .fetch_one(pool)
    .await
    .expect("a fixture step");
    SystemTime::UNIX_EPOCH + Duration::from_micros(us as u64)
}

/// T4 (B9): a PNG, a sound-only file and a truncated MP4 fail with their codes; the parts of
/// what is not a video are deleted (D7), those of an unreadable one kept for a retry (B2).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_what_is_not_a_readable_video_fails_with_its_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let png = f
        .processing(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0")
        .await;
    let sound = f.processing(&clip(&f.work.0, "mkv", None, true, 1)).await;
    let whole = clip(&f.work.0, "mp4", Some((320, 240)), false, 3);
    let truncated = f.processing(&whole[..whole.len() / 2]).await;
    assert_eq!(f.work(f.transcoder(60, None)).await, 3);
    for (video, code) in [
        (png, "video.not_a_video"),
        (sound, "video.not_a_video"),
        (truncated, "video.probe_failed"),
    ] {
        assert_eq!(
            f.status(video).await,
            ("failed".to_owned(), Some(code.to_owned())),
            "{code}"
        );
    }
    let objects = f.objects().await;
    assert!(
        objects.iter().all(|k| k.starts_with(&f.prefix(truncated))),
        "{objects:?}"
    );
    assert_eq!(
        objects.len(),
        2,
        "the unreadable video keeps both parts: {objects:?}"
    );
    assert_eq!(f.uploads().await, 1);
    assert!(f.usage().await.is_empty());
}

/// T4 (B9): a clip longer than `max_video_seconds` fails with `video.too_long`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_a_clip_longer_than_the_maximum_is_too_long(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mkv", Some((320, 240)), false, 3))
        .await;
    f.work(f.transcoder(2, None)).await;
    assert_eq!(
        f.status(video).await,
        ("failed".to_owned(), Some("video.too_long".to_owned()))
    );
}

/// T8 (B9, B13, B21): with `transcode_minutes` at 1 and 1 used this month, a new video fails
/// with `entitlement.limit_reached`; in the next month a new one succeeds and its minute is
/// recorded for that month.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_transcode_minutes_are_counted_per_calendar_month(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.limit(serde_json::json!({ "transcode_minutes": 1, "video_storage": null }))
        .await;
    let bytes = clip(&f.work.0, "mp4", Some((320, 240)), false, 3);
    let first = f.processing(&bytes).await;
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(first).await.0, "ready");
    let refused = f.processing(&bytes).await;
    f.work(f.transcoder(60, None)).await;
    assert_eq!(
        f.status(refused).await,
        (
            "failed".to_owned(),
            Some("entitlement.limit_reached".to_owned())
        )
    );
    f.clock.advance(Duration::from_secs(2 * 60 * 60));
    let april = f.processing(&bytes).await;
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(april).await.0, "ready");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1), ("2026-04-01".to_owned(), 1)]
    );
}

/// B12: outputs that take the organization past `video_storage` fail the video and are deleted;
/// the original stays.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn outputs_past_the_video_storage_limit_fail_the_video_and_are_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let bytes = clip(&f.work.0, "mp4", Some((1280, 720)), true, 3);
    f.limit(serde_json::json!({ "video_storage": bytes.len(), "transcode_minutes": null }))
        .await;
    let video = f.processing(&bytes).await;
    f.work(f.transcoder(60, None)).await;
    assert_eq!(
        f.status(video).await,
        (
            "failed".to_owned(),
            Some("entitlement.limit_reached".to_owned())
        )
    );
    let objects = f.objects().await;
    assert!(
        objects.iter().all(|k| k.contains("/upload/")),
        "{objects:?}"
    );
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1)],
        "the plan's limit is not the server's failure: the charge stays"
    );
}

/// T7 (B2, B12): ffmpeg failing fails the video `video.transcode_failed` and keeps its
/// original; a retry writes to `v2`, and once `v2` is committed the job deletes `v1`'s objects.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_retry_after_a_failure_writes_the_next_run_and_deletes_the_old_one(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 3))
        .await;
    let failing = find_on_path("false").expect("false on PATH");
    f.work(f.transcoder(60, Some(failing))).await;
    assert_eq!(
        f.status(video).await,
        (
            "failed".to_owned(),
            Some("video.transcode_failed".to_owned())
        )
    );
    assert_eq!(f.uploads().await, 1, "the original is kept for a retry");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1)],
        "B13: charged when the render started, kept when the input fails it"
    );
    assert_eq!(
        f.charge_record(video).await,
        None,
        "the input's failure keeps the charge and clears the record, not refunds it"
    );
    // What a player of run 1 would be reading had it been ready.
    let old = format!("{}/v1/master.m3u8", f.prefix(video));
    f.storage
        .store()
        .put(&f.storage.path(&old), PutPayload::from_static(b"#EXTM3U\n"))
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let retried = transcodes::retry(&mut tx, scope(f.org), (video, &[f.course]), f.now())
        .await
        .unwrap();
    assert_eq!(retried, Retried::Processing);
    f.enqueue(&mut tx, video).await;
    tx.commit().await.unwrap();
    assert_eq!(f.retries(video).await, 1);
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(video).await.0, "ready");
    let objects = f.objects().await;
    assert!(objects.iter().all(|k| k.contains("/v2/")), "{objects:?}");
    assert!(!objects.is_empty());
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 2)],
        "each run is charged"
    );
}

/// B2: only a video that failed as `video.transcode_failed`, with its upload stored, can be
/// retried.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn only_a_transcode_failure_with_its_original_can_be_retried(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f.processing(b"not a video at all").await;
    let s = scope(f.org);
    let retry = |video| {
        let pool = f.pool.clone();
        let (course, now) = (f.course, f.now());
        async move {
            let mut tx = pool.begin().await.unwrap();
            let retried = transcodes::retry(&mut tx, s, (video, &[course]), now)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            retried
        }
    };
    assert_eq!(retry(video).await, Retried::StatusInvalid, "processing");
    f.work(f.transcoder(60, None)).await;
    assert_eq!(retry(video).await, Retried::StatusInvalid, "not a video");
    assert_eq!(retry(new_id()).await, Retried::NotFound);
    sqlx::query(
        "UPDATE videos SET status = 'failed', error_code = 'video.transcode_failed' WHERE id = $1",
    )
    .bind(video)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(f.uploads().await, 0, "its parts were deleted");
    assert_eq!(retry(video).await, Retried::StatusInvalid, "no original");
}

/// A transient failure is retried by the queue; on the last attempt it fails the video
/// `video.transcode_failed`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_host_failure_is_retried_and_fails_the_video_on_the_last_attempt(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 1))
        .await;
    let missing = Some(PathBuf::from("/nonexistent/ffmpeg"));
    f.work(f.transcoder(60, missing.clone())).await;
    assert_eq!(f.status(video).await.0, "processing", "retried, not failed");
    let (attempts, error): (i32, Option<String>) =
        sqlx::query_as("SELECT attempts, last_error FROM jobs WHERE kind = 'video.transcode'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 1);
    assert!(error.unwrap().contains("could not start"));
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 0)],
        "B13: a failure on the server's side refunds the charge"
    );
    sqlx::query("UPDATE jobs SET attempts = $1, run_at = timestamptz 'epoch'")
        .bind(i32::try_from(TRANSCODE.retry.max_attempts).unwrap() - 1)
        .execute(&f.pool)
        .await
        .unwrap();
    f.clock.advance(Duration::from_secs(2 * 60 * 60));
    f.work(f.transcoder(60, missing)).await;
    assert_eq!(
        f.status(video).await,
        (
            "failed".to_owned(),
            Some("video.transcode_failed".to_owned())
        )
    );
    // The last attempt ran in April: its charge went to April and was refunded there.
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 0), ("2026-04-01".to_owned(), 0)]
    );
}

/// Security re-check (major): a run that charged and died without ending, whose lease then
/// lapsed, is refunded by the next run's start, so the video is charged once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_run_that_died_after_charging_is_refunded_by_the_next_run(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 3))
        .await;
    f.charge_without_ending(video, 1).await;
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 1)]);
    // The 10-minute lease the dead run held has lapsed; another worker claims the job, still in
    // March.
    f.clock.advance(Duration::from_secs(11 * 60));
    assert_eq!(f.work(f.transcoder(60, None)).await, 1);
    assert_eq!(f.status(video).await.0, "ready");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1)],
        "one charge: the dead run's was refunded when the next one started"
    );
    let open: Option<i32> = sqlx::query_scalar("SELECT charged_minutes FROM videos WHERE id = $1")
        .bind(video)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        open, None,
        "a ready video keeps its charge and clears the record"
    );
}

/// Coordinator decision (2026-11-19), the owner's rule as written: every retry counts toward
/// the 3, a failure on the server's side included. Here the server's side is a process whose
/// ffmpeg cannot start, which fails the video on the job's last attempt and refunds it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_retry_counts_whatever_caused_the_failure(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 1))
        .await;
    let missing = Some(PathBuf::from("/nonexistent/ffmpeg"));
    let failed = (
        "failed".to_owned(),
        Some("video.transcode_failed".to_owned()),
    );
    for round in 0..4 {
        last_attempt(&f).await;
        f.work(f.transcoder(60, missing.clone())).await;
        assert_eq!(f.status(video).await, failed, "round {round}");
        let expected = if round < 3 {
            Retried::Processing
        } else {
            Retried::LimitReached
        };
        assert_eq!(f.retry(video).await, expected, "round {round}");
    }
    assert_eq!(f.retries(video).await, 3);
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 0)],
        "each failure refunded"
    );
}

/// Re-check: a run that stops on the server's side before it knows its run (here, a process
/// without storage) refunds nothing on its retry path, since the charge on the row may be a
/// newer run's; the next run's start refunds the dead run's charge instead.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_retry_without_a_run_leaves_the_refund_to_the_next_start(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 3))
        .await;
    f.charge_without_ending(video, 1).await;
    f.work_with(None, Some(f.transcoder(60, None))).await;
    assert_eq!(f.status(video).await.0, "processing", "retried");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1)],
        "nothing refunded"
    );
    assert_eq!(f.charge_record(video).await, Some(1));
    sqlx::query("UPDATE jobs SET run_at = timestamptz 'epoch'")
        .execute(&f.pool)
        .await
        .unwrap();
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(video).await.0, "ready");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 1)],
        "charged once"
    );
}

/// Re-check: a run that lost its lease refunds only its own charge; the newer run's start has
/// already refunded it, and the newer run's own charge stays.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_lost_lease_refunds_only_its_own_runs_charge(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f.processing(b"a video two workers took").await;
    let s = scope(f.org);
    let fixture = &f;
    let take = || async move {
        let f = fixture;
        let mut tx = f.pool.begin().await.unwrap();
        let transcodes::Start::Run { run, .. } =
            transcodes::start(&mut tx, s, video, f.now()).await.unwrap()
        else {
            panic!("a run")
        };
        assert!(
            transcodes::charge(&mut tx, s, (video, run), 2, f.now())
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        run
    };
    let lost = take().await;
    let newer = take().await;
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 2)],
        "the lost run's refunded"
    );
    // What `Job::end` does for `Stop::Superseded` at the lost run.
    let mut tx = f.pool.begin().await.unwrap();
    let refunded = transcodes::refund_open(&mut tx, s, video, Some(lost))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(refunded, None);
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 2)],
        "the newer run's charge stays"
    );
    let mut tx = f.pool.begin().await.unwrap();
    let refunded = transcodes::refund_open(&mut tx, s, video, Some(newer))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(refunded, Some(2));
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 0)]);
}

/// Re-check, through the job: a run whose lease lapses while ffmpeg renders, and whose job
/// another worker then claims and charges again, learns it lost the lease at its next extension
/// and ends as `Stop::Superseded`. The newer run's charge stays and nothing more is refunded.
///
/// The render is a stand-in ffmpeg that waits, so the run stays in its race with `hold_lease`.
/// The other worker's claim and run are the queue's and the transcode repository's own calls,
/// not a second worker pass, so no second render runs. Tokio's clock is advanced by
/// `LEASE_EVERY` only once the newer run has charged, which wakes `hold_lease` for its
/// extension; the job's clock is the fixture's `ManualClock`.
#[cfg(unix)]
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_lease_lost_mid_render_ends_superseded_and_keeps_the_newer_runs_charge(pool: PgPool) {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new(pool).await;
    let rendering = f.work.0.join("rendering");
    let waiting = f.work.0.join("waiting-ffmpeg");
    let script = format!(
        "#!/bin/sh\ntouch '{}'\nexec sleep 600\n",
        rendering.display()
    );
    std::fs::write(&waiting, script).unwrap();
    std::fs::set_permissions(&waiting, std::fs::Permissions::from_mode(0o755)).unwrap();
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 3))
        .await;

    let first = f.work(f.transcoder(60, Some(waiting)));
    let takeover = async {
        // The first run has charged its minute and is rendering.
        let started = tokio::time::Instant::now();
        while !rendering.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "ffmpeg started"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(f.charge_record(video).await, Some(1));
        let lost: i32 = sqlx::query_scalar("SELECT output_run FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        // Its lease lapses, and another worker claims the job and starts and charges a newer run.
        f.clock.advance(LEASE + Duration::from_secs(60));
        let claims = queue::claim(
            &f.pool,
            &[jobs::TRANSCODE_QUEUE],
            &[(TRANSCODE.name, LEASE)],
            1,
            f.now(),
        )
        .await
        .unwrap();
        assert_eq!(
            claims
                .claimed
                .iter()
                .map(|c| c.attempts)
                .collect::<Vec<_>>(),
            [2],
            "the other worker holds the job now"
        );
        let s = scope(f.org);
        let mut tx = f.pool.begin().await.unwrap();
        let transcodes::Start::Run { run: newer, .. } =
            transcodes::start(&mut tx, s, video, f.now()).await.unwrap()
        else {
            panic!("a newer run")
        };
        assert!(
            transcodes::charge(&mut tx, s, (video, newer), 2, f.now())
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            f.usage().await,
            [("2026-03-01".to_owned(), 2)],
            "the newer run's start refunded the first run's minute"
        );
        // The first run's next extension is due.
        tokio::time::pause();
        tokio::time::advance(LEASE_EVERY).await;
        tokio::time::resume();
        (lost, newer)
    };
    let (ran, (lost, newer)) = tokio::join!(first, takeover);
    assert_eq!(ran, 1);
    assert!(newer > lost);

    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 2)],
        "only the lost run's charge was refunded"
    );
    assert_eq!(
        f.charge_record(video).await,
        Some(2),
        "the newer run's charge stays"
    );
    let (status, run): (String, i32) =
        sqlx::query_as("SELECT status, output_run FROM videos WHERE id = $1")
            .bind(video)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        (status.as_str(), run),
        ("processing", newer),
        "the lost run neither failed the video nor took its run back"
    );
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM jobs WHERE kind = 'video.transcode'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 2, "the job is still the other worker's");
}

/// Re-check: a charge made in March and refunded after a restart in April goes back to March.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refund_goes_back_to_the_month_of_its_charge(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 3))
        .await;
    f.charge_without_ending(video, 1).await;
    f.clock.advance(Duration::from_secs(2 * 60 * 60));
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(video).await.0, "ready");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 0), ("2026-04-01".to_owned(), 1)]
    );
}

/// Re-check: an ffmpeg the kernel kills with `SIGKILL` (as the out-of-memory killer does) is the
/// host's fault, through the whole job: the run is refunded and retried, the video still
/// `processing`.
#[cfg(unix)]
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_ffmpeg_killed_by_sigkill_is_refunded_and_retried(pool: PgPool) {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new(pool).await;
    let killed = f.work.0.join("killed-ffmpeg");
    std::fs::write(&killed, "#!/bin/sh\nkill -KILL $$\n").unwrap();
    std::fs::set_permissions(&killed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 1))
        .await;
    assert_eq!(f.work(f.transcoder(60, Some(killed))).await, 1);
    assert_eq!(f.status(video).await, ("processing".to_owned(), None));
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 0)], "refunded");
    let (attempts, dead): (i32, Option<String>) =
        sqlx::query_as("SELECT attempts, dead_at::text FROM jobs WHERE kind = 'video.transcode'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!((attempts, dead), (1, None), "retried by the queue");
}

/// Put the only `video.transcode` job on its last attempt, due now.
async fn last_attempt(f: &Fixture) {
    sqlx::query("UPDATE jobs SET attempts = $1, run_at = timestamptz 'epoch'")
        .bind(i32::try_from(TRANSCODE.retry.max_attempts).expect("a fixture step") - 1)
        .execute(&f.pool)
        .await
        .expect("a fixture step");
}

/// Review finding 3: a process without storage or without ffmpeg that claims the job retries it
/// and, on the last attempt, fails the video instead of leaving it processing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_process_that_cannot_transcode_fails_the_video_on_the_last_attempt(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let bytes = clip(&f.work.0, "mp4", Some((320, 240)), false, 1);
    let no_storage = f.processing(&bytes).await;
    assert_eq!(f.work_with(None, Some(f.transcoder(60, None))).await, 1);
    assert_eq!(f.status(no_storage).await.0, "processing", "retried first");
    last_attempt(&f).await;
    f.work_with(None, Some(f.transcoder(60, None))).await;
    let failed = (
        "failed".to_owned(),
        Some("video.transcode_failed".to_owned()),
    );
    assert_eq!(f.status(no_storage).await, failed);

    let no_ffmpeg = f.processing(&bytes).await;
    last_attempt(&f).await;
    f.work_with(Some(f.storage.clone()), None).await;
    assert_eq!(f.status(no_ffmpeg).await, failed);
    assert!(f.usage().await.is_empty(), "nothing was charged");
}

/// Review finding 3: a job whose lease lapsed on its last attempt is dead without its handler
/// running; `videos.cleanup` fails its video. A processing video with no job, untouched for
/// 24 hours, is failed too; one touched since is kept.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_fails_processing_videos_no_live_job_will_finish(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let bytes = b"bytes whose transcode never ends";
    let lapsed = f.processing(bytes).await;
    // The run that charged and then lost the job: its charge is still open on the row.
    f.charge_without_ending(lapsed, 1).await;
    sqlx::query(
        "UPDATE jobs SET attempts = max_attempts, run_at = timestamptz 'epoch',
                         locked_until = timestamptz 'epoch'",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(f.work(f.transcoder(60, None)).await, 0, "dead at the claim");
    let dead: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE dead_at IS NOT NULL")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(dead, 1);
    let orphan = f.processing(bytes).await;
    let recent = f.processing(bytes).await;
    sqlx::query("DELETE FROM jobs WHERE dead_at IS NULL")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE videos SET updated_at = timestamptz 'epoch' WHERE id = $1")
        .bind(orphan)
        .execute(&f.pool)
        .await
        .unwrap();
    let report = stepclimb_server::videos_cleanup::run(&f.pool, &f.storage, f.now())
        .await
        .unwrap();
    assert_eq!((report.stranded, report.failed), (2, 0));
    let failed = (
        "failed".to_owned(),
        Some("video.transcode_failed".to_owned()),
    );
    assert_eq!(f.status(lapsed).await, failed);
    assert_eq!(f.status(orphan).await, failed);
    assert_eq!(f.status(recent).await.0, "processing");
    assert_eq!(
        f.usage().await,
        [("2026-03-01".to_owned(), 0)],
        "the sweep refunds the open charge of what it fails"
    );
}

/// Owner decision (2026-11-17): a video can be retried 3 times.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_video_is_retried_at_most_three_times(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f.processing(b"a video whose transcode fails").await;
    let s = scope(f.org);
    for expected in [
        Retried::Processing,
        Retried::Processing,
        Retried::Processing,
        Retried::LimitReached,
    ] {
        sqlx::query(
            "UPDATE videos SET status = 'failed', error_code = 'video.transcode_failed'
             WHERE id = $1",
        )
        .bind(video)
        .execute(&f.pool)
        .await
        .unwrap();
        let mut tx = f.pool.begin().await.unwrap();
        let retried = transcodes::retry(&mut tx, s, (video, &[f.course]), f.now())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(retried, expected);
    }
    let retries: i32 = sqlx::query_scalar("SELECT transcode_retries FROM videos WHERE id = $1")
        .bind(video)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(retries, 3);
}

/// The note of D13 for I2: ending a failed video deletes everything under its prefix, transcode
/// outputs included.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn ending_a_failed_video_deletes_its_whole_prefix(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f.processing(b"not a video, three parts").await;
    let s = scope(f.org);
    let stray = format!("{}/v3/720p/segment_00000.ts", f.prefix(video));
    f.storage
        .store()
        .put(&f.storage.path(&stray), PutPayload::from_static(b"ts"))
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let upload = videos::lock_upload_of_video(&mut tx, s, video)
        .await
        .unwrap()
        .unwrap();
    videos::mark_terminating(&mut tx, s, upload.id, Terminating::Video, f.now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        video_parts::finish(&f.pool, &f.storage, s, upload.id)
            .await
            .unwrap()
    );
    assert!(f.objects().await.is_empty());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM videos WHERE id = $1")
        .bind(video)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// Re-check: a part missing from the store after `start` found the upload complete is the
/// server's side: retried by the queue, nothing charged, the video still `processing`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_part_lost_from_the_store_is_retried_not_failed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 1))
        .await;
    let part = f.storage.video_part_path(scope(f.org), video, 0);
    f.storage.store().delete(&part).await.unwrap();
    assert_eq!(f.work(f.transcoder(60, None)).await, 1);
    assert_eq!(f.status(video).await, ("processing".to_owned(), None));
    assert!(
        f.usage().await.is_empty(),
        "the probe never ran, nothing charged"
    );
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM jobs WHERE kind = 'video.transcode'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 1);
}

/// Re-check: an output that is not a regular file (here, a link an "ffmpeg" leaves) stops the
/// run on the server's side: refunded and retried, nothing uploaded.
#[cfg(unix)]
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_link_among_the_outputs_is_refunded_and_retried(pool: PgPool) {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new(pool).await;
    let linking = f.work.0.join("linking-ffmpeg");
    std::fs::write(&linking, "#!/bin/sh\nln -sf /etc/hostname leaked.ts\n").unwrap();
    std::fs::set_permissions(&linking, std::fs::Permissions::from_mode(0o755)).unwrap();
    let video = f
        .processing(&clip(&f.work.0, "mp4", Some((320, 240)), false, 1))
        .await;
    assert_eq!(f.work(f.transcoder(60, Some(linking))).await, 1);
    assert_eq!(f.status(video).await, ("processing".to_owned(), None));
    assert_eq!(f.usage().await, [("2026-03-01".to_owned(), 0)], "refunded");
    let objects = f.objects().await;
    assert!(
        objects.iter().all(|k| k.contains("/upload/")),
        "{objects:?}"
    );
}
