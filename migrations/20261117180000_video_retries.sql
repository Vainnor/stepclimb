-- SPDX-License-Identifier: AGPL-3.0-or-later
-- How many times an author has retried a video's transcode
-- (docs/specs/m1/08-uploaded-video.md B2; owner decision, 2026-11-17). `POST
-- /api/v1/videos/{video_id}/retry` refuses a fourth retry, since each retry charges transcode
-- minutes again (B13). Not in the data model.
ALTER TABLE videos
    ADD COLUMN transcode_retries integer NOT NULL DEFAULT 0 CHECK (transcode_retries >= 0);
