# Dirt API (Managed Credential Backend)

Last updated: 2026-02-10

`dirt-api` is the backend service for secure credential brokering.

## Why it exists

- Client apps should be install-and-use.
- Mobile/desktop/CLI must not require users to configure infra keys.
- Long-lived credentials must stay server-side.

## Security model

- Clients authenticate with Supabase access tokens (`Bearer ...`).
- Backend verifies JWT signatures against Supabase JWKS.
- Backend enforces JWT claim checks (`aud`, `iss`, `sub`, `role`, `exp`, `iat`, optional `nbf`).
- Backend returns short-lived credentials only:
  - Turso sync token
  - R2 presigned media operation URLs
- Protected endpoints apply per-user rate limits and return HTTP `429` with `Retry-After` when exceeded.

## Endpoints

- `GET /healthz`
  - Liveness probe.
- `POST /v1/sync/token` (auth required)
  - Exchanges authenticated user context for short-lived Turso token.
  - Response shape:
    - `auth_token`
    - `expires_at` (unix seconds)
    - `database_url`
- `POST /v1/media/presign/upload` (auth required)
  - Body: `object_key`, optional `content_type`
  - Returns presigned URL + method + required headers.
- `GET /v1/media/presign/download` (auth required)
  - Query: `object_key`
- `POST /v1/media/presign/delete` (auth required)
  - Body: `object_key`
- `GET /healthz`
  - Includes in-memory abuse-rate counters (`sync_allowed`, `sync_limited`, `media_allowed`, `media_limited`).

## Configuration

Use server environment variables (see `.env.example` backend section):

- Supabase verification:
  - `SUPABASE_URL`
  - `SUPABASE_JWKS_URL` (optional override)
  - `SUPABASE_JWT_ISSUER` (optional override)
  - `SUPABASE_JWT_AUDIENCE`
- Turso token broker:
  - `TURSO_API_URL`
  - `TURSO_ORGANIZATION_SLUG`
  - `TURSO_DATABASE_NAME`
  - `TURSO_DATABASE_URL`
  - `TURSO_PLATFORM_API_TOKEN` (server-only secret)
- Hardening/rate limits:
  - `AUTH_CLOCK_SKEW_SECS` (default `60`)
  - `RATE_LIMIT_WINDOW_SECS` (default `60`)
  - `SYNC_TOKEN_RATE_LIMIT_PER_WINDOW` (default `20`)
  - `MEDIA_PRESIGN_RATE_LIMIT_PER_WINDOW` (default `120`)
- Media signing (optional):
  - `R2_ACCOUNT_ID`
  - `R2_BUCKET`
  - `R2_ACCESS_KEY_ID` (server-only secret)
  - `R2_SECRET_ACCESS_KEY` (server-only secret)

## Local run

```bash
cargo run -p dirt-api
```

Default bind address: `127.0.0.1:8080`.

## Operational requirements

- Never log raw tokens or secret keys.
- Rotate `TURSO_PLATFORM_API_TOKEN` and R2 credentials periodically.
- Revoke/rotate immediately on suspected compromise.
