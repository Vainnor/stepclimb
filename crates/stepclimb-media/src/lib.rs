// SPDX-License-Identifier: AGPL-3.0-or-later
//! The ffmpeg side of uploaded video (docs/specs/m1/08-uploaded-video.md B8 to B11): what a
//! transcode runs and how it is bounded. Storage, rows and jobs are the server's; this crate
//! touches only the local files of one transcode.
//!
//! - [`tools`] finds ffmpeg and ffprobe and checks at startup that both run (B8).
//! - [`workdir`] is one transcode's private temporary directory, removed when dropped (B9, B12).
//! - [`probe`] runs ffprobe on the concatenated upload with its demuxer pinned and reads the
//!   duration, the frame size and whether there is sound (B9).
//! - [`plan`] decides the outputs from what ffprobe found (B11: the HLS ladder, the MP4, the
//!   poster, the thumbnail sprites and their WebVTT index) and builds each ffmpeg argument list.
//!   Every argument is a constant, a number the plan computed or a path inside the work
//!   directory: no title, file name or other author string reaches a command line (B10).
//! - [`run`] runs one child process with no shell, an empty environment, no stdin, the file
//!   protocol only, and a wall-clock deadline, an output size cap and a memory cap, keeping the
//!   last 2 KiB of its stderr for the server's log (B10).
//!
//! - [`served`] says which output names a playback token serves, and as what type (B18).
//!
//! Resumable uploads (tus) live in the server, which stores each chunk as its own object;
//! captions are a later issue of the same spec.

pub mod plan;
pub mod probe;
pub mod run;
pub mod served;
pub mod tools;
pub mod workdir;
