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
use stepclimb_jobs::{NewJob, Worker, backoff::Random, enqueue};
use stepclimb_media::tools::{Tools, find_on_path};
use stepclimb_server::{
    config::StorageConfig,
    jobs,
    storage::Storage,
    video_parts,
    video_transcode::{TRANSCODE, Transcoder},
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
        let registry =
            jobs::registry(Some(self.storage.clone()), Some(transcoder)).expect("a fixture step");
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
    assert!(
        f.usage().await.is_empty(),
        "nothing recorded for a failed transcode"
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
    f.work(f.transcoder(60, None)).await;
    assert_eq!(f.status(video).await.0, "ready");
    let objects = f.objects().await;
    assert!(objects.iter().all(|k| k.contains("/v2/")), "{objects:?}");
    assert!(!objects.is_empty());
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
