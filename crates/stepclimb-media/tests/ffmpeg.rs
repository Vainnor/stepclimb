// SPDX-License-Identifier: AGPL-3.0-or-later
//! The probe and the four ffmpeg runs against the dev shell's ffmpeg
//! (docs/specs/m1/08-uploaded-video.md B9 to B11; T4 to T6). Every clip is generated here from
//! ffmpeg's own `testsrc` and `sine` sources, 3 s long; nothing is downloaded and nothing is
//! read from the network.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use stepclimb_media::{
    plan::{self, Plan, Render},
    probe::{self, Container, ProbeError, Probed},
    run::{self, Invocation, Limits, RunError},
    tools::find_on_path,
    workdir::WorkDir,
};

fn ffmpeg() -> PathBuf {
    find_on_path("ffmpeg").expect("ffmpeg on PATH (the dev shell has it)")
}

fn ffprobe() -> PathBuf {
    find_on_path("ffprobe").expect("ffprobe on PATH (the dev shell has it)")
}

/// A `seconds` long `testsrc` clip of `width` x `height`, with a 440 Hz tone when `sound`, as
/// `name` (its extension picks the container) in `dir`.
fn clip(dir: &Path, name: &str, (width, height): (u32, u32), seconds: u32, sound: bool) -> PathBuf {
    let out = dir.join(name);
    let mut command = Command::new(ffmpeg());
    command.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
    ]);
    command.arg(format!(
        "testsrc=size={width}x{height}:rate=25:duration={seconds}"
    ));
    if sound {
        command.args(["-f", "lavfi", "-i"]);
        command.arg(format!("sine=frequency=440:duration={seconds}"));
        command.args(["-c:a", "aac", "-shortest"]);
    }
    command.args([
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
    ]);
    let status = command.arg(&out).status().expect("ffmpeg runs");
    assert!(status.success(), "the fixture {name} was made");
    out
}

fn work() -> WorkDir {
    WorkDir::create(&std::env::temp_dir()).expect("a work directory")
}

async fn probed(container: Container, input: &Path, dir: &Path) -> Result<Probed, ProbeError> {
    probe::probe((&ffprobe(), None), container, input, dir).await
}

/// Probe, plan and render `input` into `dir/out`: the plan.
async fn transcode(container: Container, input: &Path, dir: &Path) -> Plan {
    let Probed::Video(source) = probed(container, input, dir).await.expect("probe") else {
        panic!("a video")
    };
    let plan = Plan::new(container, source);
    let out = dir.join("out");
    std::fs::create_dir(&out).expect("a fixture step");
    let ffmpeg = ffmpeg();
    let render = Render {
        ffmpeg: &ffmpeg,
        threads: 2,
        input,
        out: &out,
        confine: None,
    };
    plan::render(&plan, render).await.expect("render");
    plan
}

/// The files under `dir`, relative and sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        for entry in std::fs::read_dir(&at).expect("a fixture step") {
            let path = entry.expect("a fixture step").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(
                    path.strip_prefix(dir)
                        .expect("a fixture step")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    found.sort();
    found
}

/// ffprobe's duration of `path` read as HLS, which is what a player does with a playlist.
fn hls_plays(playlist: &Path) -> f64 {
    let output = Command::new(ffprobe())
        .args(["-v", "error", "-protocol_whitelist", "file", "-f", "hls"])
        .args(["-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(playlist)
        .output()
        .expect("ffprobe runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("a duration")
}

/// T6 (B11): a 720p source gets 360p and 720p, each playlist plays, the MP4's `moov` comes
/// before its `mdat`, and the sprite index covers the duration.
#[tokio::test]
async fn t6_a_720p_source_gets_360p_and_720p_and_every_output() {
    let dir = work();
    let input = clip(dir.path(), "input.mp4", (1280, 720), 3, true);
    let plan = transcode(Container::Mov, &input, dir.path()).await;
    let out = dir.path().join("out");
    assert_eq!(
        files(&out),
        [
            "360p/index.m3u8",
            "360p/segment_00000.ts",
            "720p/index.m3u8",
            "720p/segment_00000.ts",
            "master.m3u8",
            "poster.jpg",
            "sprite_000.jpg",
            "thumbnails.vtt",
            "video.mp4",
        ]
    );
    let master = std::fs::read_to_string(out.join("master.m3u8")).unwrap();
    assert!(
        master.contains("360p/index.m3u8") && master.contains("720p/index.m3u8"),
        "{master}"
    );
    for rendition in ["360p", "720p"] {
        let playlist = std::fs::read_to_string(out.join(rendition).join("index.m3u8")).unwrap();
        assert!(
            !playlist.contains('/'),
            "relative segment names only: {playlist}"
        );
        let played = hls_plays(&out.join(rendition).join("index.m3u8"));
        assert!((played - 3.0).abs() < 0.2, "{rendition}: {played}");
    }
    let mp4 = std::fs::read(out.join("video.mp4")).unwrap();
    let at = |atom: &[u8]| mp4.windows(4).position(|w| w == atom).expect("the atom");
    assert!(at(b"moov") < at(b"mdat"), "faststart puts moov first");
    let index = std::fs::read_to_string(out.join("thumbnails.vtt")).unwrap();
    assert!(
        index.ends_with("00:00:00.000 --> 00:00:03.000\nsprite_000.jpg#xywh=0,0,160,90\n"),
        "{index}"
    );
    assert_eq!(plan.sprites, 1);
}

/// T6: a 1080p source gets all three renditions.
#[tokio::test]
async fn t6_a_1080p_source_gets_all_three_renditions() {
    let dir = work();
    let input = clip(dir.path(), "input.mp4", (1920, 1080), 3, false);
    transcode(Container::Mov, &input, dir.path()).await;
    let master = std::fs::read_to_string(dir.path().join("out/master.m3u8")).unwrap();
    for rendition in ["360p", "720p", "1080p"] {
        assert!(
            master.contains(&format!("{rendition}/index.m3u8")),
            "{master}"
        );
        hls_plays(&dir.path().join("out").join(rendition).join("index.m3u8"));
    }
}

/// T6: a 320x240 Matroska source gets one 240p rendition.
#[tokio::test]
async fn t6_a_240p_matroska_source_gets_a_240p_rendition() {
    let dir = work();
    let input = clip(dir.path(), "input.mkv", (320, 240), 3, true);
    let plan = transcode(Container::Matroska, &input, dir.path()).await;
    assert_eq!(plan.renditions.len(), 1);
    let master = std::fs::read_to_string(dir.path().join("out/master.m3u8")).unwrap();
    assert!(master.contains("240p/index.m3u8"), "{master}");
    assert!(master.contains("RESOLUTION=320x240"), "{master}");
}

/// T4 (B9): a PNG and a truncated MP4 are unreadable to the probe; a sound-only file has no
/// video. The probe pins the claimed container, so a Matroska file claimed as MP4 fails too.
#[tokio::test]
async fn t4_what_is_not_a_readable_video_fails_the_probe() {
    let dir = work();
    let png = dir.path().join("image.png");
    std::fs::write(
        &png,
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0",
    )
    .unwrap();
    let error = probed(Container::Mov, &png, dir.path()).await.unwrap_err();
    assert!(
        matches!(&error, ProbeError::Run(e) if e.is_input_fault()),
        "{error:?}"
    );

    let whole = std::fs::read(clip(dir.path(), "whole.mp4", (320, 240), 3, false)).unwrap();
    // The fixture's `moov` is at its end: cut it off.
    let truncated = dir.path().join("truncated.mp4");
    std::fs::write(&truncated, &whole[..whole.len() / 2]).unwrap();
    let error = probed(Container::Mov, &truncated, dir.path())
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ProbeError::Run(e) if e.is_input_fault()),
        "{error:?}"
    );

    let mkv = clip(dir.path(), "claimed.mkv", (320, 240), 1, false);
    assert!(probed(Container::Mov, &mkv, dir.path()).await.is_err());

    let sound = dir.path().join("sound.mkv");
    let status = Command::new(ffmpeg())
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
        ])
        .arg(&sound)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        probed(Container::Matroska, &sound, dir.path())
            .await
            .unwrap(),
        Probed::NoVideo
    );
}

/// T5 (B10): a playlist that names an `http://` segment, uploaded as a video, is read only
/// through the pinned demuxer and the file protocol: the probe fails and ffmpeg, even asked
/// for the HLS demuxer with the transcode's whitelist, refuses the protocol before connecting.
#[tokio::test]
async fn t5_a_crafted_playlist_reaches_no_network() {
    let dir = work();
    let input = dir.path().join("input");
    std::fs::write(
        &input,
        "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nhttp://192.0.2.1/a.ts\n#EXT-X-ENDLIST\n",
    )
    .unwrap();
    assert!(probed(Container::Mov, &input, dir.path()).await.is_err());
    let mut args: Vec<std::ffi::OsString> =
        ["-v", "error", "-protocol_whitelist", "file", "-f", "hls"]
            .into_iter()
            .map(Into::into)
            .collect();
    args.push(probe::file_url(&input));
    let program = ffprobe();
    let invocation = Invocation {
        program: &program,
        args: &args,
        dir: dir.path(),
        watch: dir.path(),
        capture_stdout: true,
        confine: None,
    };
    let limits = Limits {
        deadline: tokio::time::Instant::now() + Duration::from_secs(30),
        max_output_bytes: u64::MAX,
        max_memory_bytes: u64::MAX,
    };
    let error = run::run(invocation, limits).await.unwrap_err();
    let RunError::Failed { stderr, .. } = &error else {
        panic!("{error:?}")
    };
    assert!(
        stderr.contains("Protocol 'http' not on whitelist 'file'"),
        "{stderr}"
    );
}

/// B10: an ffmpeg run past its output cap or its deadline is killed.
#[tokio::test]
async fn an_ffmpeg_run_past_its_output_cap_or_deadline_is_killed() {
    let dir = work();
    let input = clip(dir.path(), "input.mp4", (640, 360), 10, false);
    let Probed::Video(source) = probed(Container::Mov, &input, dir.path()).await.unwrap() else {
        panic!("a video")
    };
    let plan = Plan::new(Container::Mov, source);
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let args = plan::mp4_args(&plan, &input, 1);
    let program = ffmpeg();
    let invocation = Invocation {
        program: &program,
        args: &args,
        dir: &out,
        watch: &out,
        capture_stdout: false,
        confine: None,
    };
    let generous = tokio::time::Instant::now() + Duration::from_secs(60);
    let tiny = Limits {
        deadline: generous,
        max_output_bytes: 1,
        max_memory_bytes: u64::MAX,
    };
    let error = run::run(invocation, tiny).await.unwrap_err();
    assert!(
        matches!(error, RunError::OutputTooLarge { max: 1 }),
        "{error:?}"
    );
    let late = Limits {
        deadline: tokio::time::Instant::now(),
        max_output_bytes: u64::MAX,
        max_memory_bytes: u64::MAX,
    };
    let error = run::run(invocation, late).await.unwrap_err();
    assert!(matches!(error, RunError::TimedOut), "{error:?}");
}

/// Rewrite the Matroska `Duration` element (ID `0x4489`, an 8-byte float in milliseconds) of
/// `bytes` to `ms`.
fn patch_mkv_duration(bytes: &mut [u8], ms: f64) {
    let at = bytes
        .windows(3)
        .position(|w| w == [0x44, 0x89, 0x88])
        .expect("an 8-byte Duration element");
    bytes[at + 3..at + 11].copy_from_slice(&ms.to_be_bytes());
}

/// B9, B10: a file whose container declares 1 s but holds 10 s is probed as 1 s, and every
/// output stops at 1 s, so it is billed and bounded by what was probed.
#[tokio::test]
async fn a_file_that_declares_a_shorter_duration_is_cut_at_it() {
    let dir = work();
    let path = clip(dir.path(), "long.mkv", (320, 240), 10, false);
    let mut bytes = std::fs::read(&path).expect("read the clip");
    patch_mkv_duration(&mut bytes, 1000.0);
    let input = dir.path().join("short.mkv");
    std::fs::write(&input, &bytes).expect("write the patched clip");
    let plan = transcode(Container::Matroska, &input, dir.path()).await;
    assert!(
        (plan.source.duration_s - 1.0).abs() < 0.01,
        "{}",
        plan.source.duration_s
    );
    let played = hls_plays(&dir.path().join("out/240p/index.m3u8"));
    assert!(
        played < 1.5,
        "the rendition stops at the probed 1 s: {played}"
    );
    let mp4 = Command::new(ffprobe())
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(dir.path().join("out/video.mp4"))
        .output()
        .expect("ffprobe runs");
    let mp4: f64 = String::from_utf8_lossy(&mp4.stdout)
        .trim()
        .parse()
        .expect("a duration");
    assert!(mp4 < 1.5, "the MP4 stops at the probed 1 s: {mp4}");
}

/// Spec 08 D19, D42: under `prlimit`, a source that needs more address space than its run
/// may hold (here an 8K source, two threads, under 2 GiB) fails promptly and as the input's
/// fault, not by the kernel's out-of-memory `SIGKILL`; a small source passes under the same
/// limits.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_input_past_its_address_space_fails_promptly_as_the_inputs_fault() {
    let dir = work();
    let prlimit = find_on_path("prlimit").expect("prlimit on PATH (util-linux)");
    let ffmpeg = ffmpeg();
    let cap = 2 * 1024 * 1024 * 1024;
    for (name, size, fits) in [
        ("small.mp4", (320, 240), true),
        ("big.mp4", (7680, 4320), false),
    ] {
        let input = clip(dir.path(), name, size, 1, false);
        let Probed::Video(source) = probed(Container::Mov, &input, dir.path()).await.unwrap()
        else {
            panic!("a video")
        };
        let plan = Plan::new(Container::Mov, source);
        let out = dir.path().join(format!("out-{name}"));
        std::fs::create_dir(&out).unwrap();
        let render = Render {
            ffmpeg: &ffmpeg,
            threads: 2,
            input: &input,
            out: &out,
            confine: Some((&prlimit, cap)),
        };
        let started = std::time::Instant::now();
        let rendered = plan::render(&plan, render).await;
        if fits {
            rendered.expect("a small source fits");
        } else {
            let error = rendered.expect_err("an 8K source does not fit 2 GiB");
            assert!(matches!(error, RunError::Failed { .. }), "{error:?}");
            assert!(error.is_input_fault(), "{error:?}");
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "-xerror ends it at once"
            );
        }
    }
}
