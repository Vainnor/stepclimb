// SPDX-License-Identifier: AGPL-3.0-or-later
//! Which output names a playback token serves, and as what type
//! (docs/specs/m1/08-uploaded-video.md B18).
//!
//! A token covers one video's `v<n>/` prefix. [`output`] accepts exactly the names a transcode
//! writes there ([`crate::plan`]): the master playlist, each rendition's media playlist and MPEG-TS
//! segments under `<height>p/` for a height the video has, the MP4, the poster, the sprites'
//! WebVTT index and the sprites. Every other path, `..` or another prefix included, is refused, so
//! a token reaches nothing but the outputs its video's run produced.

use crate::plan::{MASTER_PLAYLIST, MEDIA_PLAYLIST, MP4, POSTER, SPRITE_INDEX, rendition_dir};

/// The digits of a segment number: `%05d`, 6 for a video past 5 digits of 6 s segments.
const SEGMENT_DIGITS: std::ops::RangeInclusive<usize> = 5..=6;

/// The digits of a sprite number: `%03d`, 4 past 1,000 sprites.
const SPRITE_DIGITS: std::ops::RangeInclusive<usize> = 3..=4;

/// What an output is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The master playlist or a rendition's media playlist.
    Playlist,
    /// An MPEG-TS segment of a rendition.
    Segment,
    /// The MP4 offered for download.
    Mp4,
    /// The poster or a thumbnail sprite.
    Image,
    /// The sprites' WebVTT index.
    SpriteIndex,
}

impl Output {
    /// The `Content-Type` it is served as.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Playlist => "application/vnd.apple.mpegurl",
            Self::Segment => "video/mp2t",
            Self::Mp4 => "video/mp4",
            Self::Image => "image/jpeg",
            Self::SpriteIndex => "text/vtt; charset=utf-8",
        }
    }
}

/// What `name`, relative to a run's prefix, is for a video whose renditions are `heights`
/// tall, or `None` when a transcode never writes it.
#[must_use]
pub fn output(name: &str, heights: &[u32]) -> Option<Output> {
    match name {
        MASTER_PLAYLIST => return Some(Output::Playlist),
        MP4 => return Some(Output::Mp4),
        POSTER => return Some(Output::Image),
        SPRITE_INDEX => return Some(Output::SpriteIndex),
        _ => {}
    }
    if let Some(number) = name
        .strip_prefix("sprite_")
        .and_then(|rest| rest.strip_suffix(".jpg"))
    {
        return digits(number, SPRITE_DIGITS).then_some(Output::Image);
    }
    let (dir, file) = name.split_once('/')?;
    if !heights.iter().any(|&h| rendition_dir(h) == dir) {
        return None;
    }
    if file == MEDIA_PLAYLIST {
        return Some(Output::Playlist);
    }
    let number = file.strip_prefix("segment_")?.strip_suffix(".ts")?;
    digits(number, SEGMENT_DIGITS).then_some(Output::Segment)
}

/// Whether `text` is only ASCII digits, as many as `count` allows.
fn digits(text: &str, count: std::ops::RangeInclusive<usize>) -> bool {
    count.contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outputs_a_transcode_writes_are_served_with_their_types() {
        let heights = [360, 720];
        let cases = [
            ("master.m3u8", Output::Playlist),
            ("360p/index.m3u8", Output::Playlist),
            ("720p/segment_00000.ts", Output::Segment),
            ("720p/segment_123456.ts", Output::Segment),
            ("video.mp4", Output::Mp4),
            ("poster.jpg", Output::Image),
            ("sprite_000.jpg", Output::Image),
            ("sprite_1000.jpg", Output::Image),
            ("thumbnails.vtt", Output::SpriteIndex),
        ];
        for (name, expected) in cases {
            assert_eq!(output(name, &heights), Some(expected), "{name}");
        }
    }

    #[test]
    fn every_other_path_is_refused() {
        let heights = [360, 720];
        for name in [
            "",
            "/master.m3u8",
            "../master.m3u8",
            "360p/../../v1/master.m3u8",
            "360p/../720p/index.m3u8",
            "1080p/index.m3u8",
            "0360p/index.m3u8",
            "360p/index.m3u8/x",
            "360p/segment_0000.ts",
            "360p/segment_0000000.ts",
            "360p/segment_-0001.ts",
            "360p/segment_00001.mp4",
            "360p/segment_00001.ts/",
            "360p//segment_00001.ts",
            "sprite_00.jpg",
            "sprite_00a.jpg",
            "sprite_00000.jpg",
            "upload/0",
            "MASTER.M3U8",
            "master.m3u8%00",
            "v1/master.m3u8",
        ] {
            assert_eq!(output(name, &heights), None, "{name}");
        }
        assert_eq!(output("360p/index.m3u8", &[]), None, "no renditions");
    }
}
