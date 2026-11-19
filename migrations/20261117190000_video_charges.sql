-- SPDX-License-Identifier: AGPL-3.0-or-later
-- The transcode minutes a video's current run has charged (docs/specs/m1/08-uploaded-video.md
-- B13). Not in the data model.
--
-- `charged_minutes` and `charged_at` are written in the transaction that charges a run's minutes
-- and cleared when the run ends: kept by `ready` or a failure the input caused, refunded by a
-- failure on the server's side. A run that never ended (a crash, a lost lease) leaves them set,
-- and the next run's start, or `videos.cleanup`, refunds them, so a restart never charges twice.
ALTER TABLE videos
    ADD COLUMN charged_minutes integer CHECK (charged_minutes >= 0),
    ADD COLUMN charged_at timestamptz,
    ADD CONSTRAINT videos_charge_whole CHECK ((charged_minutes IS NULL) = (charged_at IS NULL));
