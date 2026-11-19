// SPDX-License-Identifier: AGPL-3.0-or-later
//! ffprobe on an uploaded video (docs/specs/m1/08-uploaded-video.md B9, D10's note for I2).
//!
//! The demuxer is pinned with `-f` from the container the upload's first bytes claimed
//! ([`Container`]), so a file that only looks like a QuickTime movie cannot have ffprobe pick
//! another demuxer, such as HLS, which opens the URLs a playlist names. The input is read through
//! the `file` protocol only (`-protocol_whitelist file`, and a `file:` input URL, so a path is
//! never read as another protocol). ffmpeg gets the same input options ([`input_options`]).

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::run::{self, Confinement, Invocation, Limits, RunError};

/// How long ffprobe may take on one file.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// The most memory ffprobe may hold: 1 GiB.
pub const PROBE_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;

/// The largest width or height a source may have: 8192 pixels; past it ffprobe's answer is
/// refused as `video.probe_failed` (B9).
pub const MAX_SIDE: u32 = 8192;

/// The most one side may exceed the other: 16 times. A thinner frame is refused as
/// `video.probe_failed`, so a thumbnail's height stays between 10 and 2,560 pixels.
pub const MAX_ASPECT: u32 = 16;

/// The container an upload's first bytes claim (`stepclimb_core::sniff::video`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// MP4 or QuickTime: ffmpeg's `mov` demuxer.
    Mov,
    /// Matroska or WebM.
    Matroska,
}

impl Container {
    /// The container of a sniffed video type, or `None` for any other type.
    #[must_use]
    pub fn from_sniffed(media_type: &str) -> Option<Self> {
        match media_type {
            "video/mp4" | "video/quicktime" => Some(Self::Mov),
            "video/webm" | "video/x-matroska" => Some(Self::Matroska),
            _ => None,
        }
    }

    /// The demuxer `-f` names.
    #[must_use]
    pub fn demuxer(self) -> &'static str {
        match self {
            Self::Mov => "mov,mp4,m4a,3gp,3g2,mj2",
            Self::Matroska => "matroska,webm",
        }
    }
}

/// `file:` and `path`: an input URL ffmpeg can only read as a local file.
#[must_use]
pub fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path);
    url
}

/// The options before an input of `container`: the file protocol only and the pinned demuxer.
#[must_use]
pub fn input_options(container: Container) -> Vec<OsString> {
    ["-protocol_whitelist", "file", "-f", container.demuxer()]
        .into_iter()
        .map(OsString::from)
        .collect()
}

/// ffprobe's argument list for `input` of `container`.
#[must_use]
pub fn args(container: Container, input: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-v", "error", "-hide_banner"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.extend(input_options(container));
    args.extend(
        [
            "-show_entries",
            "format=duration:stream=codec_type,width,height",
            "-of",
            "json",
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(file_url(input));
    args
}

/// What ffprobe found in a file that has a video stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    /// Seconds, more than 0.
    pub duration_s: f64,
    /// Of the first video stream, in pixels.
    pub width: u32,
    pub height: u32,
    /// Whether the file has an audio stream.
    pub has_audio: bool,
}

/// What ffprobe found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Probed {
    Video(Source),
    /// It read the file and found no video stream (`video.not_a_video`).
    NoVideo,
}

/// Why a probe gave no answer.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// ffprobe did not run to success; [`RunError::is_input_fault`] says whose fault.
    #[error(transparent)]
    Run(#[from] RunError),
    /// ffprobe succeeded, but its answer has no usable duration or frame size
    /// (`video.probe_failed`).
    #[error("ffprobe found no duration or frame size")]
    Unreadable,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

/// Read ffprobe's JSON answer.
pub fn parse(json: &[u8]) -> Result<Probed, ProbeError> {
    let answer: Answer = serde_json::from_slice(json).map_err(|_| ProbeError::Unreadable)?;
    let kind = |stream: &&Stream, kind: &str| stream.codec_type.as_deref() == Some(kind);
    let Some(video) = answer.streams.iter().find(|s| kind(s, "video")) else {
        return Ok(Probed::NoVideo);
    };
    let duration_s = answer
        .format
        .and_then(|f| f.duration)
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or(ProbeError::Unreadable)?;
    let (Some(width), Some(height)) = (video.width, video.height) else {
        return Err(ProbeError::Unreadable);
    };
    let (long, short) = (width.max(height), width.min(height));
    if short == 0 || long > MAX_SIDE || long / short > MAX_ASPECT {
        return Err(ProbeError::Unreadable);
    }
    Ok(Probed::Video(Source {
        duration_s,
        width,
        height,
        has_audio: answer.streams.iter().any(|s| kind(&s, "audio")),
    }))
}

/// The open files ffprobe may hold (`RLIMIT_NOFILE`).
pub const PROBE_OPEN_FILES: u64 = 64;

/// Run `ffprobe` on `input` of `container`, from the work directory `work`, under `prlimit` when
/// given: its address space at [`PROBE_MEMORY_BYTES`], no file larger than 1 MiB (it writes
/// none), [`PROBE_OPEN_FILES`] and [`PROBE_TIMEOUT`] of CPU time.
pub async fn probe(
    (ffprobe, prlimit): (&Path, Option<&Path>),
    container: Container,
    input: &Path,
    work: &Path,
) -> Result<Probed, ProbeError> {
    let args = args(container, input);
    let confine = prlimit.map(|prlimit| Confinement {
        prlimit,
        address_space: PROBE_MEMORY_BYTES,
        file_size: 1024 * 1024,
        open_files: PROBE_OPEN_FILES,
        cpu_seconds: PROBE_TIMEOUT.as_secs(),
    });
    let invocation = Invocation {
        program: ffprobe,
        args: &args,
        dir: work,
        watch: work,
        capture_stdout: true,
        confine,
    };
    let limits = Limits {
        deadline: tokio::time::Instant::now() + PROBE_TIMEOUT,
        // ffprobe writes nothing; the input already under `work` is its only size.
        max_output_bytes: u64::MAX,
        max_memory_bytes: PROBE_MEMORY_BYTES,
    };
    let ended = run::run(invocation, limits).await?;
    parse(&ended.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffed_types_map_to_a_pinned_demuxer() {
        assert_eq!(Container::from_sniffed("video/mp4"), Some(Container::Mov));
        assert_eq!(
            Container::from_sniffed("video/quicktime"),
            Some(Container::Mov)
        );
        assert_eq!(
            Container::from_sniffed("video/webm"),
            Some(Container::Matroska)
        );
        assert_eq!(
            Container::from_sniffed("video/x-matroska"),
            Some(Container::Matroska)
        );
        for other in ["image/png", "application/vnd.apple.mpegurl", ""] {
            assert_eq!(Container::from_sniffed(other), None, "{other}");
        }
        assert_eq!(Container::Mov.demuxer(), "mov,mp4,m4a,3gp,3g2,mj2");
        assert_eq!(Container::Matroska.demuxer(), "matroska,webm");
    }

    #[test]
    fn the_probe_pins_the_demuxer_and_the_file_protocol() {
        let args = args(Container::Matroska, Path::new("/work/input"));
        let args: Vec<_> = args.iter().map(|a| a.to_string_lossy()).collect();
        let at = |flag: &str| args.iter().position(|a| a == flag).expect(flag);
        assert_eq!(args[at("-protocol_whitelist") + 1], "file");
        assert_eq!(args[at("-f") + 1], "matroska,webm");
        assert_eq!(args.last().unwrap(), "file:/work/input");
        assert!(
            at("-f") < args.len() - 1,
            "the demuxer comes before the input"
        );
    }

    #[test]
    fn an_answer_with_a_video_stream_gives_its_size_duration_and_sound() {
        let json = br#"{"streams":[{"codec_type":"video","width":1280,"height":720},
            {"codec_type":"audio"}],"format":{"duration":"3.000000"}}"#;
        assert_eq!(
            parse(json).unwrap(),
            Probed::Video(Source {
                duration_s: 3.0,
                width: 1280,
                height: 720,
                has_audio: true
            })
        );
        let silent = br#"{"streams":[{"codec_type":"video","width":320,"height":240}],
            "format":{"duration":"25.5"}}"#;
        let Probed::Video(source) = parse(silent).unwrap() else {
            panic!("a video")
        };
        assert!(!source.has_audio);
    }

    #[test]
    fn an_answer_without_a_video_stream_is_no_video() {
        let json = br#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"3.0"}}"#;
        assert_eq!(parse(json).unwrap(), Probed::NoVideo);
        assert_eq!(parse(br#"{"streams":[]}"#).unwrap(), Probed::NoVideo);
    }

    #[test]
    fn the_largest_and_thinnest_accepted_frames_pass() {
        for (w, h) in [(8192, 4320), (8192, 512), (512, 8192), (2, 2)] {
            let json = format!(
                r#"{{"streams":[{{"codec_type":"video","width":{w},"height":{h}}}],"format":{{"duration":"3"}}}}"#
            );
            assert!(
                matches!(parse(json.as_bytes()), Ok(Probed::Video(_))),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn an_answer_without_a_duration_or_size_is_unreadable() {
        for json in [
            &br#"not json"#[..],
            br#"{"streams":[{"codec_type":"video","width":1,"height":1}]}"#,
            br#"{"streams":[{"codec_type":"video","width":1,"height":1}],"format":{"duration":"N/A"}}"#,
            br#"{"streams":[{"codec_type":"video","width":1,"height":1}],"format":{"duration":"0"}}"#,
            br#"{"streams":[{"codec_type":"video","width":1,"height":1}],"format":{"duration":"inf"}}"#,
            br#"{"streams":[{"codec_type":"video"}],"format":{"duration":"3"}}"#,
            br#"{"streams":[{"codec_type":"video","width":0,"height":720}],"format":{"duration":"3"}}"#,
            br#"{"streams":[{"codec_type":"video","width":8193,"height":720}],"format":{"duration":"3"}}"#,
            br#"{"streams":[{"codec_type":"video","width":1280,"height":65535}],"format":{"duration":"3"}}"#,
            br#"{"streams":[{"codec_type":"video","width":3400,"height":200}],"format":{"duration":"3"}}"#,
        ] {
            assert!(
                matches!(parse(json), Err(ProbeError::Unreadable)),
                "{}",
                String::from_utf8_lossy(json)
            );
        }
    }
}
