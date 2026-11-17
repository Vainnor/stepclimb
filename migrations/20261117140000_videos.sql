-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Videos, the courses they are attached to, their tus uploads, their chapters and the transcode
-- minutes each organization used (docs/specs/m1/08-uploaded-video.md B1, B2, B4 to B6, B13,
-- Data).
--
-- A video belongs to the organization, not to a course: `course_videos` attaches it to courses,
-- so a course copy can attach the same stored renditions (Q6). An upload in progress is a
-- `video_uploads` row whose `received` bytes are stored as one object per chunk under
-- `orgs/<org_id>/videos/<video_id>/upload/<offset>`. `terminating` marks an upload whose parts are
-- being deleted, `parts` to keep the (failed) video or `video` to delete it too: no chunk is
-- taken for it any more, and `videos.cleanup` finishes the delete if the server stops in the
-- middle.
CREATE TABLE videos (
    id             uuid        PRIMARY KEY,
    org_id         uuid        NOT NULL REFERENCES organizations (id),
    source         text        NOT NULL CHECK (source IN ('upload', 'youtube', 'vimeo', 'oembed', 'url')),
    title          text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    file_id        uuid,
    url            text,
    status         text        NOT NULL CHECK (status IN ('uploading', 'processing', 'ready', 'failed')),
    error_code     text,
    duration_s     numeric(10, 3) CHECK (duration_s >= 0),
    renditions     jsonb       NOT NULL DEFAULT '{}'::jsonb,
    poster_key     text,
    output_run     integer     NOT NULL DEFAULT 0,
    storage_bytes  bigint      NOT NULL DEFAULT 0 CHECK (storage_bytes >= 0),
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, id),
    CHECK ((status = 'failed') = (error_code IS NOT NULL))
);

CREATE TABLE course_videos (
    org_id      uuid        NOT NULL,
    course_id   uuid        NOT NULL,
    video_id    uuid        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (course_id, video_id),
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE
);

-- `VideoTarget` reads the courses of a video.
CREATE INDEX course_videos_video ON course_videos (org_id, video_id);

CREATE TABLE video_uploads (
    id            uuid        PRIMARY KEY,
    org_id        uuid        NOT NULL,
    video_id      uuid        NOT NULL UNIQUE,
    course_id     uuid        NOT NULL,
    length        bigint      NOT NULL CHECK (length > 0),
    received      bigint      NOT NULL DEFAULT 0 CHECK (received >= 0 AND received <= length),
    terminating   text        CHECK (terminating IN ('parts', 'video')),
    created_by    uuid        NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, course_id) REFERENCES courses (org_id, id),
    FOREIGN KEY (org_id, created_by) REFERENCES users (org_id, id)
);

CREATE TABLE video_chapters (
    org_id      uuid        NOT NULL,
    video_id    uuid        NOT NULL,
    at_s        numeric(10, 3) NOT NULL CHECK (at_s >= 0),
    title       text        NOT NULL CHECK (btrim(title) <> '' AND char_length(title) <= 200),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (video_id, at_s),
    FOREIGN KEY (org_id, video_id) REFERENCES videos (org_id, id) ON DELETE CASCADE
);

CREATE TABLE transcode_usage (
    org_id      uuid        NOT NULL REFERENCES organizations (id),
    month       date        NOT NULL CHECK (extract(day FROM month) = 1),
    minutes     integer     NOT NULL CHECK (minutes >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, month)
);
