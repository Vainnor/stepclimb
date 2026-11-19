// SPDX-License-Identifier: AGPL-3.0-or-later
//! Playback sessions (docs/specs/m1/08-uploaded-video.md B17, B18, Q5): the prefix token that
//! lets a player fetch one ready video's outputs for 2 hours, and the two routes that issue it.
//!
//! A token seals `{ org_id, video_id, run, expires_at }` with the encryption key ring under the
//! context `media_url`, as a signed file URL's token is sealed under `file_url`
//! (docs/specs/m1/06-files.md B16; [`crate::file_urls::seal_under`]). Its holder can neither read
//! nor change it, and a file URL's token never opens as one. It covers the run's whole `v<n>/`
//! prefix: the session's URLs are `/media/<token>/<name>`, and a playlist names its segments
//! relatively, so every segment URL carries the same token and no playlist is rewritten.
//! [`crate::media_delivery`] serves them.
//!
//! `POST /api/v1/items/{item_id}/video-session` takes `Require<perm::CourseView, ItemTarget>` and
//! the item's visibility and lock check ([`crate::file_urls::unlocked_item`]).
//! `POST /api/v1/videos/{video_id}/preview-session` takes `Require<perm::CourseViewAll,
//! VideoTarget>`, for the authoring screen. Both take [`Storage`] after their `Require`, so only
//! an allowed caller learns that storage is not configured (B19), and answer 409
//! `video.not_ready` for a video that is not `ready`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    grants::Denied,
    id::Uuid,
    items,
    perm::{CourseView, CourseViewAll},
    video_playback, videos,
};
use stepclimb_media::plan::{MASTER_PLAYLIST, MP4, POSTER, SPRITE_INDEX};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::{ApiError, FieldError},
    file_urls::{open_under, seal_under, unlocked_item},
    openapi::problem_responses,
    outline,
    require::{ItemTarget, Require, VideoTarget, code::VIDEO_NOT_FOUND, not_found_as},
    storage::Storage,
    timestamp,
    videos::{Chapter, video_error},
};

/// How long a playback token is valid after it is issued (Q5).
pub const TTL: Duration = Duration::from_secs(2 * 60 * 60);

/// The path every playback URL starts with; `{MEDIA_PATH}/{token}/{name}`.
pub const MEDIA_PATH: &str = "/media";

/// The encryption context of a token: a ring's ciphertext for any other purpose, a signed file
/// URL's included, never opens as one.
const CONTEXT: &[u8] = b"media_url";

/// The codes this module produces besides the targets' 404s.
pub mod code {
    /// The video is not `ready`: nothing to play yet, or its transcode failed (409).
    pub const NOT_READY: &str = "video.not_ready";
    /// The item is not a `video` item (a 422 field code at the body's root).
    pub const NOT_A_VIDEO: &str = "item.not_a_video";
}

problem_responses! {
    VideoSessionNotFound => "`item.not_found`: no item with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the item or its module is unpublished, and you do not hold `course.view_all` at the course. `video.not_found`: the item's video no longer exists. `org.not_found`: the request's organization cannot be determined.",
    VideoSessionForbidden => "`permission.denied`: you do not hold `course.view` at a scope containing the course. `item.locked`: the item is locked for you; `reasons` says what it waits for. `auth.csrf_refused`: the request did not come from this site.",
    VideoSessionInvalid => "`request.invalid`: `errors` names the body's root with `item.not_a_video`: the item is not a `video` item.",
    VideoNotReady => "`video.not_ready`: the video has nothing to play yet: it is uploading or processing, or its transcode failed.",
    PreviewForbidden => "`permission.denied`: you can see a course the video is attached to but do not hold `course.view_all` at a scope containing any of them. `auth.csrf_refused`: the request did not come from this site.",
    MediaStorageNotConfigured => "`storage.not_configured`: the server has no file storage; `detail` names `STEPCLIMB_STORAGE_URL`. `org.not_found`: the request's organization cannot be determined.",
}

/// What a token holds, as sealed. The names are short: the token travels in every segment's URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaGrant {
    #[serde(rename = "o")]
    pub org_id: Uuid,
    #[serde(rename = "v")]
    pub video_id: Uuid,
    /// The transcode run whose `v<n>/` outputs it serves.
    #[serde(rename = "n")]
    pub run: i32,
    /// Seconds since the Unix epoch.
    #[serde(rename = "e")]
    pub expires_at: u64,
}

impl MediaGrant {
    /// The grant for run `run` of the video `video_id` of the scope's organization, valid for
    /// [`TTL`] from `now`, counted in whole seconds.
    fn issue(scope: OrgScope, video_id: Uuid, run: i32, now: SystemTime) -> Result<Self, ApiError> {
        let since_epoch = now
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ApiError::internal(&error))?;
        Ok(Self {
            org_id: scope.org_id(),
            video_id,
            run,
            expires_at: since_epoch.as_secs() + TTL.as_secs(),
        })
    }

    /// Whether the grant is still valid at `now`: strictly before `expires_at`.
    #[must_use]
    pub fn valid_at(&self, now: SystemTime) -> bool {
        now.duration_since(UNIX_EPOCH)
            .is_ok_and(|since| since.as_secs() < self.expires_at)
    }

    /// The seconds from `now` until the grant expires, 0 when it has.
    #[must_use]
    pub fn seconds_left(&self, now: SystemTime) -> u64 {
        let since = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.expires_at.saturating_sub(since)
    }
}

/// `grant` sealed with `keys` under the `media_url` context, base64url-encoded without padding.
pub fn seal(keys: &KeyRing, grant: &MediaGrant) -> Result<String, ApiError> {
    seal_under(keys, CONTEXT, grant)
}

/// The grant `token` seals, or `None` when it does not open with `keys` under the `media_url`
/// context or does not hold a grant. Expiry is the caller's to check ([`MediaGrant::valid_at`]).
#[must_use]
pub fn open(keys: &KeyRing, token: &str) -> Option<MediaGrant> {
    open_under(keys, CONTEXT, token)
}

/// A caption track of a playback session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SessionCaption {
    /// A BCP 47 language tag.
    pub lang: String,
    /// Where its WebVTT file is served.
    pub url: String,
}

/// Where a player fetches one ready video, until `expires_at`.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct VideoSession {
    /// The HLS master playlist. Every playlist and segment it leads to carries the same token;
    /// anyone holding a URL of the session can fetch the video until it expires.
    pub hls_url: String,
    /// The MP4 for download, served as an attachment.
    pub mp4_url: String,
    /// A JPEG from 10% into the video.
    pub poster_url: String,
    /// The WebVTT index of the thumbnail sprites, whose cues name the sprites relatively.
    pub thumbnails_url: String,
    /// The video's caption tracks, one per language.
    pub captions: Vec<SessionCaption>,
    /// By `at_s`.
    pub chapters: Vec<Chapter>,
    /// The length in seconds.
    pub duration_s: f64,
    /// RFC 3339, UTC: 2 hours after issue. A URL of the session answers 404 after it; ask for a
    /// new session and resume.
    #[schema(format = DateTime)]
    pub expires_at: String,
}

/// The session of the video `video_id` of the scope's organization: 404 `video.not_found` when it
/// is gone, 409 `video.not_ready` unless it is `ready`.
async fn session(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<VideoSession, ApiError> {
    let hidden = || not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND);
    let video = videos::get(&mut *conn, scope, video_id)
        .await
        .map_err(video_error)?
        .ok_or_else(hidden)?;
    let output = video_playback::output(&mut *conn, scope, video_id)
        .await
        .map_err(video_error)?
        .ok_or_else(hidden)?;
    if output.status != videos::Status::Ready {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            code::NOT_READY,
            "The video has nothing to play yet.",
        ));
    }
    let grant = MediaGrant::issue(scope, video_id, output.run, state.clock.now())?;
    let token = seal(&state.keys, &grant)?;
    let url = |name: &str| format!("{}{MEDIA_PATH}/{token}/{name}", state.public_url);
    let expires_at = timestamp::rfc3339(UNIX_EPOCH + Duration::from_secs(grant.expires_at))
        .map_err(|error| ApiError::internal(&error))?;
    Ok(VideoSession {
        hls_url: url(MASTER_PLAYLIST),
        mp4_url: url(MP4),
        poster_url: url(POSTER),
        thumbnails_url: url(SPRITE_INDEX),
        captions: Vec::new(),
        chapters: video
            .chapters
            .into_iter()
            .map(|c| Chapter {
                at_s: c.at_s,
                title: c.title,
            })
            .collect(),
        duration_s: video.duration_s.unwrap_or_default(),
        expires_at,
    })
}

/// A playback session for a `video` item's video. A caller without `course.view_all` gets it only
/// for a published item of a published module that is unlocked for them.
#[utoipa::path(
    post,
    path = "/api/v1/items/{item_id}/video-session",
    operation_id = "createItemVideoSession",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("item_id" = String, Path, format = "uuid", description = "The item's id.")),
    responses(
        (status = 200, description = "URLs that play the item's video for 2 hours", body = VideoSession),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = VideoSessionForbidden),
        (status = 404, response = VideoSessionNotFound),
        (status = 409, response = VideoNotReady),
        (status = 422, response = VideoSessionInvalid),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = MediaStorageNotConfigured),
    )
)]
pub async fn create_item_video_session(
    require: Require<CourseView, ItemTarget>,
    _storage: Storage,
    State(state): State<AppState>,
) -> Result<Json<VideoSession>, ApiError> {
    let scope = require.grants().scope();
    let course_id = require.target().course.id;
    let (mut tx, item) = unlocked_item(&state, &require).await?;
    let video_id = match (item.kind, item.ref_id) {
        (items::Kind::Video, Some(id)) => id,
        _ => {
            return Err(ApiError::invalid(vec![FieldError::at(
                &[],
                code::NOT_A_VIDEO,
            )]));
        }
    };
    // `items::create` keeps a video item's video attached to its course, and a video an item
    // names cannot be detached; checked again, so an item never plays another course's video.
    let attached = videos::courses_of(&mut *tx, scope, video_id)
        .await
        .map_err(video_error)?;
    if !attached.contains(&course_id) {
        return Err(not_found_as(Denied::CourseNotFound, VIDEO_NOT_FOUND));
    }
    let session = session(&state, &mut tx, scope, video_id).await?;
    outline::end(tx).await?;
    Ok(Json(session))
}

/// A playback session for any video of a course, for its staff (the authoring screen).
#[utoipa::path(
    post,
    path = "/api/v1/videos/{video_id}/preview-session",
    operation_id = "createVideoPreviewSession",
    tag = "videos",
    extensions(("x-stepclimb-mutates" = json!(true))),
    params(("video_id" = String, Path, format = "uuid", description = "The video's id.")),
    responses(
        (status = 200, description = "URLs that play the video for 2 hours", body = VideoSession),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 401, response = crate::openapi::Unauthenticated),
        (status = 403, response = PreviewForbidden),
        (status = 404, response = crate::videos::VideoNotFound),
        (status = 409, response = VideoNotReady),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = MediaStorageNotConfigured),
    )
)]
pub async fn create_video_preview_session(
    require: Require<CourseViewAll, VideoTarget>,
    _storage: Storage,
    State(state): State<AppState>,
) -> Result<Json<VideoSession>, ApiError> {
    let scope = require.grants().scope();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|error| ApiError::internal(&error))?;
    Ok(Json(
        session(&state, &mut conn, scope, require.target().id).await?,
    ))
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    use super::*;
    use crate::file_urls;

    fn ring() -> KeyRing {
        KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap()
    }

    fn grant() -> MediaGrant {
        MediaGrant {
            org_id: Uuid::from_u128(1),
            video_id: Uuid::from_u128(2),
            run: 3,
            expires_at: 1_763_007_200,
        }
    }

    #[test]
    fn a_sealed_grant_opens_to_itself_and_hides_the_video() {
        let keys = ring();
        let token = seal(&keys, &grant()).unwrap();
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{token}"
        );
        assert_eq!(open(&keys, &token), Some(grant()));
        assert!(!token.contains(&Uuid::from_u128(2).simple().to_string()));
    }

    #[test]
    fn a_changed_token_or_a_file_urls_token_does_not_open() {
        let keys = ring();
        let token = seal(&keys, &grant()).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(&token).unwrap();
        bytes[0] ^= 1;
        assert_eq!(open(&keys, &URL_SAFE_NO_PAD.encode(&bytes)), None);
        // The same JSON sealed as a signed file URL's token.
        let as_file = file_urls::seal_under(&keys, b"file_url", &grant()).unwrap();
        assert_eq!(open(&keys, &as_file), None);
        assert_eq!(open(&keys, "not a token"), None);
    }

    #[test]
    fn a_grant_is_valid_until_its_expiry_second() {
        let g = grant();
        let at = |s| UNIX_EPOCH + Duration::from_secs(s);
        assert!(g.valid_at(at(g.expires_at - 1)));
        assert!(!g.valid_at(at(g.expires_at)));
        assert_eq!(g.seconds_left(at(g.expires_at - 7200)), 7200);
        assert_eq!(g.seconds_left(at(g.expires_at + 1)), 0);
    }
}
