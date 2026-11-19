// SPDX-License-Identifier: AGPL-3.0-or-later
//! What playback of a video reads (docs/specs/m1/08-uploaded-video.md B17, B18): its status, the
//! transcode run whose outputs are served (`v<n>/`, B12) and the heights of its renditions. A
//! playback session names the run in its token, and each request under the token reads this
//! again, so a video deleted or transcoded again since stops serving the old token. The query
//! filters by `org_id = scope.org_id()`.

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope,
    videos::{Error, Status},
};

/// A video's served outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: Status,
    /// The transcode run whose outputs are under `v<run>/`; 0 before any run.
    pub run: i32,
    /// The heights of its HLS renditions, shortest first; empty until a transcode succeeds.
    pub heights: Vec<u32>,
}

impl Output {
    /// Whether run `run` is the one a `ready` video serves.
    #[must_use]
    pub fn serves(&self, run: i32) -> bool {
        self.status == Status::Ready && self.run == run
    }
}

/// The outputs of the video `video_id` of the scope's organization, or `None`.
pub async fn output(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<Option<Output>, Error> {
    let row = sqlx::query!(
        r#"SELECT status, output_run,
                  ARRAY(SELECT h::int FROM jsonb_array_elements_text(
                            COALESCE(renditions->'heights', '[]'::jsonb)) AS h) AS "heights!"
           FROM videos WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        video_id,
    )
    .fetch_optional(db)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let stored = |column| Error::Stored {
        id: video_id,
        column,
    };
    Ok(Some(Output {
        status: Status::parse(&row.status).ok_or_else(|| stored("status"))?,
        run: row.output_run,
        heights: row
            .heights
            .into_iter()
            .map(u32::try_from)
            .collect::<Result<_, _>>()
            .map_err(|_| stored("renditions"))?,
    }))
}
