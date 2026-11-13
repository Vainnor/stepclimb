# 0019. Which MinIO image the Compose stack uses

- **Status:** Proposed
- **Date:** 2026-11-12
- **Decider:** the owner (open)
- **Related:** `deploy/compose/compose.yaml`; `docs/specs/m0/14-compose-deploy.md`

## Context

The plan's Compose option includes optional MinIO. Checked on 12 Nov 2026 from this machine:
Docker Hub's API returns 404 for the `minio/minio` repository and `docker manifest inspect
minio/minio:latest` answers `denied` (older `RELEASE.*` tags too);
`docker manifest inspect` reports "no such manifest" for `quay.io/minio/minio:latest` and
`bitnami/minio:latest`. `cgr.dev/chainguard/minio:latest` resolves; Chainguard's free tier
publishes only `latest`. [verified: commands run 2026-11-12]

## Options

### Option A: Chainguard's MinIO image, pinned by digest

Works today; the digest is the pin. Updates mean reading Chainguard's `latest` and re-pinning, with no version tag to read.

### Option B: build MinIO from source into our own image

MinIO's own route for the community edition. A build and an image to maintain.

### Option C: another S3-compatible server (Garage, SeaweedFS)

Actively published images; `object_store` speaks S3 to any of them. Not MinIO, which the prompt named; needs a check of the S3 features M1 uses (multipart uploads, presigned URLs).

## Recommendation

Option A for development now, revisited in the M1 media spec against Option C,
because production storage is the self-hoster's choice (`object_store` supports local disk, S3, R2,
GCS and Azure) and Compose only needs a local S3 for tests and demos.

## Decision

Not decided yet.

## Consequences

The development stack runs `cgr.dev/chainguard/minio:latest` pinned by digest until the owner
decides. Nothing reads object storage before M1, so the choice can wait for the media spec.
