// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a transcode produces and the ffmpeg runs that produce it
//! (docs/specs/m1/08-uploaded-video.md B10, B11, Q2).
//!
//! [`Plan::new`] decides the outputs from what ffprobe found: the HLS renditions of [`LADDER`] no
//! taller than the source (one at the source's own height below 360p), an MP4 at 720p or the
//! source height if lower, a poster from 10% into the video, and thumbnail sprites. [`render`]
//! runs ffmpeg four times into the output directory (HLS, MP4, poster, sprites) under one
//! deadline of 4 x the duration plus 10 minutes (B10), then writes the sprites' WebVTT index.
//!
//! Every output name is a constant of this module, so a playlist names only relative segment
//! names and the server can upload the directory as it is. Each run drops the source's metadata
//! and chapters, so no tag an author's file carries is copied into what learners download.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use crate::probe::{Container, Source, file_url, input_options};
use crate::run::{self, Confinement, Invocation, Limits, RunError};

/// One HLS rendition: its height and video bitrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendition {
    /// Pixels; even.
    pub height: u32,
    /// Bits per second.
    pub video_bitrate: u32,
}

/// The HLS ladder (B11, Q2): 360p at 800 kbit/s, 720p at 2.8 Mbit/s, 1080p at 5 Mbit/s.
pub const LADDER: [Rendition; 3] = [
    Rendition {
        height: 360,
        video_bitrate: 800_000,
    },
    Rendition {
        height: 720,
        video_bitrate: 2_800_000,
    },
    Rendition {
        height: 1080,
        video_bitrate: 5_000_000,
    },
];

/// AAC stereo at 128 kbit/s in every output with sound.
pub const AUDIO_BITRATE: u32 = 128_000;

/// The MP4's height when the source is at least this tall.
pub const MP4_MAX_HEIGHT: u32 = 720;

/// The length of an HLS segment.
pub const SEGMENT_SECONDS: u32 = 6;

/// One thumbnail every this many seconds, this wide, packed into sprites of this many columns
/// and rows (B11).
pub const THUMB_EVERY_SECONDS: u32 = 10;
pub const THUMB_WIDTH: u32 = 160;
pub const SPRITE_COLUMNS: u32 = 10;
pub const SPRITE_ROWS: u32 = 10;

/// The largest single allocation ffmpeg may make (`-max_alloc`): 256 MiB, above any frame of a
/// 4K source and below what a forged frame size would ask for.
pub const MAX_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

/// The largest picture a decoder may allocate (`-max_pixels`): 8192 x 4320, an 8K frame
/// (owner decision on D19, 2026-11-17). A forged frame size past it fails the run before the
/// decoder allocates for it.
pub const MAX_PIXELS: u64 = 8192 * 4320;

/// The most resident memory one ffmpeg run may hold: 4 GiB.
pub const MAX_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The ffmpeg threads of a run when `transcode_threads` is 0. Capped rather than left to ffmpeg,
/// which picks one per core, because `RLIMIT_AS` counts each thread's stack and buffers
/// (docs/specs/m1/08-uploaded-video.md D42).
pub const DEFAULT_THREADS: u32 = 8;

/// The address space ([`address_space`]) of a run with up to [`DEFAULT_THREADS`] threads: 5 GiB.
/// The worst render measured at 8 threads, an 8K source to the whole ladder, peaked at 3.8 GiB
/// of virtual memory, a 1080p source at 2.8 GiB (ffmpeg 9.0.2, x86_64).
pub const BASE_ADDRESS_SPACE: u64 = 5 * 1024 * 1024 * 1024;

/// The address space each thread past [`DEFAULT_THREADS`] adds: 192 MiB, above the 111 to 149 MiB
/// per thread measured between 8 and 16 threads.
pub const ADDRESS_SPACE_PER_THREAD: u64 = 192 * 1024 * 1024;

/// The most files an ffmpeg run may hold open (`RLIMIT_NOFILE`): its input, three renditions'
/// playlists and segments, and its own descriptors fit well inside.
pub const OPEN_FILES: u64 = 256;

/// The threads a run uses: `transcode_threads`, or [`DEFAULT_THREADS`] when that is 0.
#[must_use]
pub fn effective_threads(threads: u32) -> u32 {
    if threads == 0 {
        DEFAULT_THREADS
    } else {
        threads
    }
}

/// The address space (`RLIMIT_AS`) an ffmpeg run of `threads` threads ([`effective_threads`])
/// starts under: [`BASE_ADDRESS_SPACE`], plus [`ADDRESS_SPACE_PER_THREAD`] for each thread past
/// [`DEFAULT_THREADS`]. It must stay below the container's memory, so an input that needs more
/// fails in ffmpeg instead of drawing the kernel's out-of-memory kill.
#[must_use]
pub fn address_space(threads: u32) -> u64 {
    let extra = u64::from(effective_threads(threads).saturating_sub(DEFAULT_THREADS));
    BASE_ADDRESS_SPACE.saturating_add(extra.saturating_mul(ADDRESS_SPACE_PER_THREAD))
}

/// The output cap: 64 MiB, plus 3 MB per second of video, about twice the 1.5 MB/s the whole
/// ladder, the MP4 and their sound add up to.
pub const OUTPUT_BASE_BYTES: u64 = 64 * 1024 * 1024;
pub const OUTPUT_BYTES_PER_SECOND: u64 = 3_000_000;

/// The output names, relative to the output directory.
pub const MASTER_PLAYLIST: &str = "master.m3u8";
pub const MEDIA_PLAYLIST: &str = "index.m3u8";
pub const MP4: &str = "video.mp4";
pub const POSTER: &str = "poster.jpg";
pub const SPRITE_INDEX: &str = "thumbnails.vtt";

/// The directory of a rendition: `360p`, `720p`, ...
#[must_use]
pub fn rendition_dir(height: u32) -> String {
    format!("{height}p")
}

/// The name of sprite `index`: `sprite_000.jpg`, ...
#[must_use]
pub fn sprite_name(index: u32) -> String {
    format!("sprite_{index:03}.jpg")
}

/// `n` rounded down to an even number, at least 2.
fn even_down(n: u32) -> u32 {
    (n - n % 2).max(2)
}

/// The renditions of a source `source_height` pixels tall (B11).
#[must_use]
pub fn renditions(source_height: u32) -> Vec<Rendition> {
    let fitting: Vec<_> = LADDER
        .into_iter()
        .filter(|r| r.height <= source_height)
        .collect();
    if fitting.is_empty() {
        vec![Rendition {
            height: even_down(source_height),
            video_bitrate: LADDER[0].video_bitrate,
        }]
    } else {
        fitting
    }
}

/// The bitrate for a picture `height` tall: the ladder's for the tallest step not above it.
fn bitrate_for(height: u32) -> u32 {
    LADDER
        .iter()
        .rev()
        .find(|r| r.height <= height)
        .map_or(LADDER[0].video_bitrate, |r| r.video_bitrate)
}

/// Every output of one transcode.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub container: Container,
    pub source: Source,
    pub renditions: Vec<Rendition>,
    pub mp4: Rendition,
    /// Seconds from the start.
    pub poster_at_s: f64,
    /// One thumbnail's size; the height keeps the source's aspect, rounded to even.
    pub thumb_width: u32,
    pub thumb_height: u32,
    /// How many thumbnails, one per [`THUMB_EVERY_SECONDS`] begun.
    pub thumbs: u32,
    /// How many sprite files.
    pub sprites: u32,
}

impl Plan {
    /// The plan for `source` read through `container`.
    #[must_use]
    pub fn new(container: Container, source: Source) -> Self {
        let mp4_height = even_down(source.height).min(MP4_MAX_HEIGHT);
        let (w, h) = (u64::from(source.width), u64::from(source.height));
        let scaled = u64::from(THUMB_WIDTH)
            .saturating_mul(h)
            .saturating_add(w / 2)
            / w.max(1);
        // At most 160 x height / width + 1 for any source ffprobe reported.
        let thumb_height = u32::try_from(scaled + scaled % 2)
            .unwrap_or(u32::MAX)
            .max(2);
        let every = f64::from(THUMB_EVERY_SECONDS);
        // A duration ffprobe gave is finite and positive; past 2^32 thumbnails it saturates.
        let thumbs = ((source.duration_s / every).ceil() as u32).max(1);
        let per_sprite = SPRITE_COLUMNS * SPRITE_ROWS;
        Self {
            container,
            source,
            renditions: renditions(source.height),
            mp4: Rendition {
                height: mp4_height,
                video_bitrate: bitrate_for(mp4_height),
            },
            poster_at_s: source.duration_s * 0.1,
            thumb_width: THUMB_WIDTH,
            thumb_height,
            thumbs,
            sprites: thumbs.div_ceil(per_sprite),
        }
    }

    /// B10's wall-clock limit for every run of the transcode together: 4 x the duration plus
    /// 10 minutes.
    #[must_use]
    pub fn time_budget(&self) -> Duration {
        Duration::from_secs_f64(self.source.duration_s * 4.0) + Duration::from_secs(600)
    }

    /// The most bytes the output directory may hold.
    #[must_use]
    pub fn max_output_bytes(&self) -> u64 {
        // A duration ffprobe gave is finite and positive; the product saturates.
        let per_second =
            (self.source.duration_s.ceil() as u64).saturating_mul(OUTPUT_BYTES_PER_SECOND);
        OUTPUT_BASE_BYTES.saturating_add(per_second)
    }
}

fn strings(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

/// The options of every run up to and including the input: quiet, no prompt, `-xerror`,
/// `-max_alloc`, `-max_pixels`, the input options, `seek_s` when given, then the input.
///
/// `-xerror` ends the run at its first error. Without it, ffmpeg whose allocation fails under
/// `RLIMIT_AS` logs the error and then waits on its other threads for good (measured with
/// ffmpeg 9.0.2), holding the job until its deadline; with it, the run exits at once, the
/// input's fault.
fn head(plan: &Plan, input: &Path, seek_s: Option<f64>) -> Vec<OsString> {
    let mut args = strings(&[
        "-nostdin",
        "-hide_banner",
        "-nostats",
        "-loglevel",
        "error",
        "-y",
        "-xerror",
    ]);
    args.extend(strings(&["-max_alloc", &MAX_ALLOC_BYTES.to_string()]));
    args.extend(input_options(plan.container));
    args.extend(strings(&["-max_pixels", &MAX_PIXELS.to_string()]));
    if let Some(at) = seek_s {
        args.extend(strings(&["-ss", &format!("{at:.3}")]));
    }
    args.push("-i".into());
    args.push(file_url(input));
    args.extend(strings(&["-map_metadata", "-1", "-map_chapters", "-1"]));
    args
}

/// `-t` at the probed duration: an output stops there, so a file whose container declares a
/// shorter duration than it holds is transcoded, billed and bounded by what ffprobe reported
/// (B9, B10).
fn until_duration(args: &mut Vec<OsString>, plan: &Plan) {
    args.extend(strings(&["-t", &format!("{:.3}", plan.source.duration_s)]));
}

/// `-threads` at [`effective_threads`]: always set, so the threads, and with them the address
/// space a run needs, never grow with the host's cores.
fn threads(args: &mut Vec<OsString>, threads: u32) {
    let threads = effective_threads(threads).to_string();
    args.extend(strings(&["-threads", &threads]));
}

fn audio(args: &mut Vec<OsString>) {
    let rate = AUDIO_BITRATE.to_string();
    args.extend(strings(&["-c:a", "aac", "-b:a", &rate, "-ac", "2"]));
}

const H264: [&str; 6] = [
    "-c:v",
    "libx264",
    "-profile:v",
    "high",
    "-preset",
    "veryfast",
];

/// The HLS run: every rendition from one decode, keyframes on segment boundaries, a media
/// playlist per rendition directory and the master playlist.
#[must_use]
pub fn hls_args(plan: &Plan, input: &Path, thread_count: u32) -> Vec<OsString> {
    let mut args = head(plan, input, None);
    let n = plan.renditions.len();
    let splits: String = (0..n).map(|i| format!("[s{i}]")).collect();
    let mut graph = format!("[0:v:0]split={n}{splits}");
    for (i, r) in plan.renditions.iter().enumerate() {
        graph.push_str(&format!(
            ";[s{i}]scale=-2:{},format=yuv420p[v{i}]",
            r.height
        ));
    }
    args.extend(strings(&["-filter_complex", &graph]));
    for i in 0..n {
        args.extend(strings(&["-map", &format!("[v{i}]")]));
        if plan.source.has_audio {
            args.extend(strings(&["-map", "0:a:0"]));
        }
    }
    args.extend(strings(&H264));
    let keyframes = format!("expr:gte(t,n_forced*{SEGMENT_SECONDS})");
    args.extend(strings(&[
        "-sc_threshold",
        "0",
        "-force_key_frames",
        &keyframes,
    ]));
    let mut streams = Vec::new();
    for (i, r) in plan.renditions.iter().enumerate() {
        let (rate, buffer) = (
            r.video_bitrate.to_string(),
            (2 * r.video_bitrate).to_string(),
        );
        args.extend(strings(&[
            &format!("-b:v:{i}"),
            &rate,
            &format!("-maxrate:v:{i}"),
            &rate,
        ]));
        args.extend(strings(&[&format!("-bufsize:v:{i}"), &buffer]));
        let name = rendition_dir(r.height);
        streams.push(if plan.source.has_audio {
            format!("v:{i},a:{i},name:{name}")
        } else {
            format!("v:{i},name:{name}")
        });
    }
    if plan.source.has_audio {
        audio(&mut args);
    }
    threads(&mut args, thread_count);
    until_duration(&mut args, plan);
    let segment = SEGMENT_SECONDS.to_string();
    args.extend(strings(&[
        "-f",
        "hls",
        "-hls_time",
        &segment,
        "-hls_playlist_type",
        "vod",
        "-hls_segment_type",
        "mpegts",
        "-hls_flags",
        "independent_segments",
        "-hls_segment_filename",
        "%v/segment_%05d.ts",
        "-master_pl_name",
        MASTER_PLAYLIST,
        "-var_stream_map",
        &streams.join(" "),
        &format!("%v/{MEDIA_PLAYLIST}"),
    ]));
    args
}

/// The MP4 run: the download copy with its index (`moov`) first.
#[must_use]
pub fn mp4_args(plan: &Plan, input: &Path, thread_count: u32) -> Vec<OsString> {
    let mut args = head(plan, input, None);
    args.extend(strings(&["-map", "0:v:0"]));
    if plan.source.has_audio {
        args.extend(strings(&["-map", "0:a:0"]));
    }
    let scale = format!("scale=-2:{},format=yuv420p", plan.mp4.height);
    let (rate, buffer) = (
        plan.mp4.video_bitrate.to_string(),
        (2 * plan.mp4.video_bitrate).to_string(),
    );
    args.extend(strings(&["-vf", &scale]));
    args.extend(strings(&H264));
    args.extend(strings(&[
        "-b:v", &rate, "-maxrate", &rate, "-bufsize", &buffer,
    ]));
    if plan.source.has_audio {
        audio(&mut args);
    }
    threads(&mut args, thread_count);
    until_duration(&mut args, plan);
    args.extend(strings(&["-movflags", "+faststart", "-f", "mp4", MP4]));
    args
}

/// The poster run: one JPEG at the MP4's height from 10% into the video.
#[must_use]
pub fn poster_args(plan: &Plan, input: &Path, thread_count: u32) -> Vec<OsString> {
    let mut args = head(plan, input, Some(plan.poster_at_s));
    let scale = format!("scale=-2:{},format=yuvj420p", plan.mp4.height);
    args.extend(strings(&[
        "-map",
        "0:v:0",
        "-frames:v",
        "1",
        "-vf",
        &scale,
        "-q:v",
        "3",
    ]));
    threads(&mut args, thread_count);
    args.extend(strings(&["-f", "image2", "-update", "1", POSTER]));
    args
}

/// The sprites run: one frame every 10 s, 160 px wide, tiled 10 x 10 per JPEG. `eof_action=pass`
/// keeps the first frame of a video shorter than 10 s, which `fps` alone drops; with it, a video
/// of `d` seconds gives `ceil(d / 10)` frames, the count [`Plan::thumbs`] indexes.
#[must_use]
pub fn sprite_args(plan: &Plan, input: &Path, thread_count: u32) -> Vec<OsString> {
    let mut args = head(plan, input, None);
    let filter = format!(
        "fps=1/{THUMB_EVERY_SECONDS}:eof_action=pass,scale={}:{},format=yuvj420p,tile={SPRITE_COLUMNS}x{SPRITE_ROWS}",
        plan.thumb_width, plan.thumb_height
    );
    args.extend(strings(&["-map", "0:v:0", "-vf", &filter, "-q:v", "5"]));
    threads(&mut args, thread_count);
    until_duration(&mut args, plan);
    args.extend(strings(&[
        "-f",
        "image2",
        "-start_number",
        "0",
        "sprite_%03d.jpg",
    ]));
    args
}

/// `seconds` as a WebVTT timestamp, `hh:mm:ss.mmm`.
fn timestamp(seconds: f64) -> String {
    // A duration ffprobe gave is finite and positive; past u64 milliseconds it saturates.
    let ms = (seconds * 1000.0).round() as u64;
    let (h, m, s, ms) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// The WebVTT index of the sprites: one cue per thumbnail, naming its sprite and region
/// (`sprite_000.jpg#xywh=x,y,w,h`), the last cue ending at the duration. Region offsets are
/// computed in `u64` and saturate, so no frame size ffprobe reported can overflow them.
#[must_use]
pub fn sprite_index(plan: &Plan) -> String {
    let mut vtt = String::from("WEBVTT\n");
    let per_sprite = SPRITE_COLUMNS * SPRITE_ROWS;
    let every = f64::from(THUMB_EVERY_SECONDS);
    let (w, h) = (u64::from(plan.thumb_width), u64::from(plan.thumb_height));
    for i in 0..plan.thumbs {
        let start = f64::from(i) * every;
        let end = (start + every).min(plan.source.duration_s);
        let (sprite, at) = (i / per_sprite, i % per_sprite);
        let x = u64::from(at % SPRITE_COLUMNS).saturating_mul(w);
        let y = u64::from(at / SPRITE_COLUMNS).saturating_mul(h);
        vtt.push_str(&format!(
            "\n{} --> {}\n{}#xywh={x},{y},{w},{h}\n",
            timestamp(start),
            timestamp(end),
            sprite_name(sprite),
        ));
    }
    vtt
}

/// One of the four argument list builders [`render`] runs.
type ArgsBuilder = fn(&Plan, &Path, u32) -> Vec<OsString>;

/// Where and how [`render`] runs ffmpeg.
#[derive(Clone, Copy, Debug)]
pub struct Render<'a> {
    pub ffmpeg: &'a Path,
    /// `transcode_threads`; 0 means [`DEFAULT_THREADS`].
    pub threads: u32,
    /// The concatenated upload.
    pub input: &'a Path,
    /// The empty directory the outputs go to.
    pub out: &'a Path,
    /// util-linux `prlimit` and the address space each run starts under ([`address_space`]);
    /// `None` runs ffmpeg without OS limits, as the crate's own tests of the arguments do.
    pub confine: Option<(&'a Path, u64)>,
}

impl Render<'_> {
    /// The OS limits of one run of `plan`: the address space, any one output file no larger
    /// than the whole output cap, [`OPEN_FILES`], and CPU seconds for every thread working
    /// through the whole time budget.
    fn confinement(&self, plan: &Plan) -> Option<Confinement<'_>> {
        let (prlimit, address_space) = self.confine?;
        let threads = u64::from(effective_threads(self.threads));
        Some(Confinement {
            prlimit,
            address_space,
            file_size: plan.max_output_bytes(),
            open_files: OPEN_FILES,
            cpu_seconds: plan.time_budget().as_secs().saturating_mul(threads),
        })
    }
}

/// Run the four ffmpeg runs of `plan` into `render.out`, under one deadline of
/// [`Plan::time_budget`] from now, the output and memory caps and, through `prlimit`, the OS
/// limits of [`Render::confine`], then write the sprite index.
pub async fn render(plan: &Plan, render: Render<'_>) -> Result<(), RunError> {
    let limits = Limits {
        deadline: Instant::now() + plan.time_budget(),
        max_output_bytes: plan.max_output_bytes(),
        max_memory_bytes: MAX_MEMORY_BYTES,
    };
    let confine = render.confinement(plan);
    let builders: [ArgsBuilder; 4] = [hls_args, mp4_args, poster_args, sprite_args];
    for build in builders {
        let args = build(plan, render.input, render.threads);
        let invocation = Invocation {
            program: render.ffmpeg,
            args: &args,
            dir: render.out,
            watch: render.out,
            capture_stdout: false,
            confine,
        };
        run::run(invocation, limits).await?;
    }
    std::fs::write(render.out.join(SPRITE_INDEX), sprite_index(plan))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(width: u32, height: u32, duration_s: f64) -> Source {
        Source {
            duration_s,
            width,
            height,
            has_audio: true,
        }
    }

    fn heights(plan: &Plan) -> Vec<u32> {
        plan.renditions.iter().map(|r| r.height).collect()
    }

    #[test]
    fn the_ladder_keeps_renditions_no_taller_than_the_source() {
        let plan = |w, h| Plan::new(Container::Mov, source(w, h, 3.0));
        assert_eq!(heights(&plan(1280, 720)), [360, 720]);
        assert_eq!(heights(&plan(1920, 1080)), [360, 720, 1080]);
        assert_eq!(heights(&plan(3840, 2160)), [360, 720, 1080]);
        assert_eq!(heights(&plan(854, 480)), [360]);
        assert_eq!(heights(&plan(320, 240)), [240]);
        assert_eq!(
            heights(&plan(321, 241)),
            [240],
            "odd heights round down to even"
        );
        assert_eq!(plan(320, 240).renditions[0].video_bitrate, 800_000);
    }

    #[test]
    fn the_mp4_is_720p_or_the_source_height_if_lower() {
        let mp4 = |w, h| Plan::new(Container::Mov, source(w, h, 3.0)).mp4;
        assert_eq!(
            mp4(1920, 1080),
            Rendition {
                height: 720,
                video_bitrate: 2_800_000
            }
        );
        assert_eq!(
            mp4(854, 480),
            Rendition {
                height: 480,
                video_bitrate: 800_000
            }
        );
        assert_eq!(
            mp4(320, 240),
            Rendition {
                height: 240,
                video_bitrate: 800_000
            }
        );
    }

    #[test]
    fn thumbnails_keep_the_aspect_and_fill_sprites_of_one_hundred() {
        let plan = Plan::new(Container::Mov, source(1280, 720, 25.0));
        assert_eq!((plan.thumb_width, plan.thumb_height), (160, 90));
        assert_eq!((plan.thumbs, plan.sprites), (3, 1));
        let four_three = Plan::new(Container::Mov, source(320, 240, 1000.5));
        assert_eq!(four_three.thumb_height, 120);
        assert_eq!((four_three.thumbs, four_three.sprites), (101, 2));
        assert!((four_three.poster_at_s - 100.05).abs() < 1e-9);
    }

    #[test]
    fn the_address_space_grows_only_past_the_default_threads() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert_eq!(effective_threads(0), 8);
        assert_eq!(effective_threads(3), 3);
        for threads in [0, 1, 8] {
            assert_eq!(address_space(threads), 5 * GIB, "{threads}");
        }
        assert_eq!(address_space(16), 5 * GIB + 8 * 192 * 1024 * 1024);
        assert_eq!(address_space(64), 5 * GIB + 56 * 192 * 1024 * 1024);
    }

    #[test]
    fn the_budgets_follow_the_duration() {
        let plan = Plan::new(Container::Mov, source(1280, 720, 30.0));
        assert_eq!(plan.time_budget(), Duration::from_secs(120 + 600));
        assert_eq!(plan.max_output_bytes(), 64 * 1024 * 1024 + 90_000_000);
    }

    #[test]
    fn the_sprite_index_covers_the_duration_with_each_region() {
        let plan = Plan::new(Container::Mov, source(1280, 720, 25.0));
        assert_eq!(
            sprite_index(&plan),
            "WEBVTT\n\
             \n00:00:00.000 --> 00:00:10.000\nsprite_000.jpg#xywh=0,0,160,90\n\
             \n00:00:10.000 --> 00:00:20.000\nsprite_000.jpg#xywh=160,0,160,90\n\
             \n00:00:20.000 --> 00:00:25.000\nsprite_000.jpg#xywh=320,0,160,90\n"
        );
        let long = Plan::new(Container::Mov, source(320, 240, 3725.25));
        let index = sprite_index(&long);
        assert!(
            index.contains("\n01:02:00.000 --> 01:02:05.250\nsprite_003.jpg#xywh="),
            "{index}"
        );
        assert!(
            index.contains("sprite_001.jpg#xywh=0,0,160,120"),
            "the 101st thumbnail"
        );
        assert!(
            index.contains("sprite_000.jpg#xywh=1440,1080,160,120"),
            "the 100th"
        );
    }

    /// T5 (B10): every argument is a constant, a number or a path inside the work directory;
    /// the title and the uploaded file's name are never inputs to the plan.
    #[test]
    fn every_run_reads_one_local_file_through_the_pinned_demuxer() {
        let plan = Plan::new(Container::Matroska, source(1920, 1080, 3.0));
        let input = Path::new("/work/stepclimb-transcode-1/input");
        for args in [
            hls_args(&plan, input, 2),
            mp4_args(&plan, input, 2),
            poster_args(&plan, input, 0),
            sprite_args(&plan, input, 0),
        ] {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let at = |flag: &str| args.iter().position(|a| a == flag).expect(flag);
            assert_eq!(args[at("-protocol_whitelist") + 1], "file");
            assert_eq!(args[at("-f") + 1], "matroska,webm");
            assert!(at("-f") < at("-i"), "the demuxer is an input option");
            assert_eq!(args[at("-i") + 1], "file:/work/stepclimb-transcode-1/input");
            assert_eq!(args.iter().filter(|a| *a == "-i").count(), 1, "one input");
            assert_eq!(args[at("-max_alloc") + 1], "268435456");
            assert_eq!(args[at("-max_pixels") + 1], "35389440");
            assert!(
                at("-max_pixels") < at("-i"),
                "a decoder option of the input"
            );
            assert_eq!(args[at("-map_metadata") + 1], "-1");
            assert!(args.iter().all(|a| !a.contains("://")), "{args:?}");
        }
    }

    #[test]
    fn the_hls_run_maps_each_rendition_with_its_sound_and_bitrate() {
        let plan = Plan::new(Container::Mov, source(1280, 720, 3.0));
        let args: Vec<String> = hls_args(&plan, Path::new("/w/input"), 0)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let after = |flag: &str| args[args.iter().position(|a| a == flag).expect(flag) + 1].clone();
        assert_eq!(
            after("-filter_complex"),
            "[0:v:0]split=2[s0][s1];[s0]scale=-2:360,format=yuv420p[v0];[s1]scale=-2:720,format=yuv420p[v1]"
        );
        assert_eq!(
            after("-var_stream_map"),
            "v:0,a:0,name:360p v:1,a:1,name:720p"
        );
        assert_eq!(after("-b:v:1"), "2800000");
        assert_eq!(after("-hls_time"), "6");
        assert_eq!(
            after("-t"),
            "3.000",
            "the outputs stop at the probed duration"
        );
        assert!(
            args.iter().position(|a| a == "-t") > args.iter().position(|a| a == "-i"),
            "an output option"
        );
        assert_eq!(after("-threads"), "8", "0 means the default of 8");
        assert!(
            args.contains(&"-xerror".to_owned()),
            "ends at the first error"
        );
        let silent = Plan::new(
            Container::Mov,
            Source {
                has_audio: false,
                ..source(320, 240, 3.0)
            },
        );
        let args: Vec<String> = hls_args(&silent, Path::new("/w/input"), 3)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"v:0,name:240p".to_owned()), "{args:?}");
        assert!(!args.contains(&"0:a:0".to_owned()));
        assert!(args.windows(2).any(|w| w == ["-threads", "3"]));
    }
}
