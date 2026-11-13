compose := "docker compose -f deploy/compose/compose.yaml"

# Start PostgreSQL and MinIO for local development
up:
    {{ compose }} up -d --wait postgres minio

# Stop the Compose services (data volumes are kept)
down:
    {{ compose }} --profile stack down

# Build the server image and start the whole stack on :58080
up-stack:
    {{ compose }} --profile stack up -d --wait --build

# Playwright smoke test against the Compose stack (rebuilds the server image)
e2e: up-stack
    pnpm --filter web e2e
