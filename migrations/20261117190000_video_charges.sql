-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The transcode minutes a video's current run has charged, and who caused its last failure
-- (docs/specs/m1/08-uploaded-video.md B2, B13). Not in the data model.
--
-- `charged_minutes` and `charged_at` are written in the transaction that charges a run's minutes
-- and cleared when the run ends: kept by `ready` or a failure the input caused, refunded by a
-- failure on the server's side. A run that never ended (a crash, a lost lease) leaves them set,
-- and the next run's start, or `videos.cleanup`, refunds them, so a restart never charges twice.
-- `failed_by_server` marks a failure on the server's side, whose retry does not count toward the
-- video's 3 retries.
ALTER TABLE videos
    ADD COLUMN charged_minutes integer CHECK (charged_minutes >= 0),
    ADD COLUMN charged_at timestamptz,
    ADD COLUMN failed_by_server boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT videos_charge_whole CHECK ((charged_minutes IS NULL) = (charged_at IS NULL));
