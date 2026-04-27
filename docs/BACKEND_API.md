# Backend API

The `dirt-api` service is a small axum HTTP server that mediates note
sync between clients and a Turso (libSQL) database. It exposes three
routes — one health probe and two authenticated note endpoints — and
nothing else.

- **Base URL** in production: `https://dirt-api.vercel.app`
- **Auth**: `Authorization: Bearer <DIRT_CLIENT_TOKEN>` on every
  `/v1/*` request. The token must match `DIRT_SERVER_TOKEN` set on the
  server. There is no per-user identity in Phase 1; every accepted
  token resolves to the solo-phase user sentinel.
- **Content type**: `application/json` on push; pull is a `GET`.
- **Time**: every timestamp is a Unix-millisecond `i64`. Server-stamped
  timestamps are authoritative — clients never write
  `server_updated_at_ms`.

## Routes

### `GET /healthz`

Liveness probe. No auth, no body.

```bash
curl -s https://dirt-api.vercel.app/healthz
# ok
```

Returns `200 OK` with the literal text `ok` while the process is alive
and able to accept connections. It does *not* verify Turso reachability
— that surfaces lazily on the first `/v1/notes/*` request as
`TURSO_UNREACHABLE`.

### `POST /v1/notes/push`

Client pushes a batch of locally-mutated notes. The server stamps each
accepted row with `server_updated_at_ms` and returns the per-id
results. The request body is hard-capped at 8 MiB (`PUSH_BODY_LIMIT`);
oversized payloads short-circuit with `413` before reaching the
handler.

**Request**

```json
{
  "notes": [
    {
      "id": "01932a12-aaaa-7000-8000-000000000abc",
      "content": "draft text",
      "created_at_ms": 1713800000000,
      "client_updated_at_ms": 1713800500000,
      "deleted_at_ms": null
    }
  ]
}
```

- `notes`: up to **500** items per batch. Larger batches are rejected
  with `BATCH_TOO_LARGE`.
- `id`: client-generated UUID v7. Re-pushes of the same `id` are
  idempotent — each row is upserted.
- Duplicate `id` within a single batch: server keeps the last
  occurrence (last-wins in-batch), applies once, returns one entry.
- `client_updated_at_ms`: client wall clock at write time. Stored as a
  hint for UI; never used for ordering or conflict resolution.
- `deleted_at_ms`: tombstone timestamp. `null` for live notes.
- `user_id` is **not** accepted in the body — the server derives it
  from the bearer token.

**Response (200)**

```json
{
  "results": [
    {
      "id": "01932a12-aaaa-7000-8000-000000000abc",
      "applied": true,
      "server_updated_at_ms": 1713800600100
    }
  ],
  "server_time_ms": 1713800600000
}
```

Each entry corresponds to one input id. The client uses
`results[i].server_updated_at_ms` to update the local
`server_updated_at` column and clear the row from `pending_sync` (only
when `applied = true`). A mixed-result batch (some applied, some
skipped) is supported — each row is an independent upsert.

**Example**

```bash
curl -X POST https://dirt-api.vercel.app/v1/notes/push \
  -H "Authorization: Bearer $DIRT_CLIENT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "notes": [{
      "id": "01932aaa-0000-7000-8000-000000000abc",
      "content": "first phase1 note",
      "created_at_ms": 1777021110411,
      "client_updated_at_ms": 1777021110411,
      "deleted_at_ms": null
    }]
  }'
```

### `GET /v1/notes/pull?cursor={opaque}&limit={n}`

Paginated incremental pull, ordered by
`(server_updated_at, id)` so a misbehaving client clock can't poison
the order.

**Query parameters**

- `cursor`: opaque server-issued token. Omit on first call. The
  server interprets a missing cursor as "from the beginning"
  (`server_updated_at_ms = 0`). On subsequent pages, pass the
  `next_cursor` value from the previous response verbatim. The token
  format is implementation-private — clients must not parse it.
- `limit`: number of notes per page. Server clamps to `[1, 1000]`;
  default `500`.

**Response (200)**

```json
{
  "notes": [
    {
      "id": "01932a12-aaaa-7000-8000-000000000abc",
      "content": "text",
      "created_at_ms": 1713800000000,
      "client_updated_at_ms": 1713800500000,
      "server_updated_at_ms": 1713800600100,
      "deleted_at_ms": null
    }
  ],
  "server_time_ms": 1713800600500,
  "has_more": false,
  "next_cursor": "eyJzdWEiOjE3MTM4MDA2MDAxMDAsImlkIjoiMDE5MzJhMTIuLi4ifQ"
}
```

- `has_more = false` plus `next_cursor` set means the client should
  persist the cursor for the next sync but stop paging.
- Tombstoned rows are returned with `deleted_at_ms` populated so
  clients can apply the tombstone locally. The merge resolver in
  `dirt_core::sync::merge` decides what to do with them.

**Example**

```bash
curl -s "https://dirt-api.vercel.app/v1/notes/pull?limit=10" \
  -H "Authorization: Bearer $DIRT_CLIENT_TOKEN" | jq
```

## Error envelope

Every error response — including `4xx` middleware rejections and `5xx`
upstream failures — follows the same JSON shape:

```json
{
  "error": {
    "code": "BATCH_TOO_LARGE",
    "message": "batch has 612 notes; limit is 500",
    "cause": "batch has 612 notes; limit is 500",
    "fix": "Split the batch into groups of at most 500 notes and retry.",
    "retry_after_secs": null
  }
}
```

Clients should render `cause` and `fix` together — `fix` is the
actionable instruction, `cause` is the diagnostic detail.

`retry_after_secs` is reserved for Phase 2's per-user rate limiter; in
Phase 1 it's always `null`.

### Error code table

| HTTP | `code` | Meaning | Client action |
|---|---|---|---|
| 401 | `UNAUTHORIZED` | `Authorization` header missing or token didn't match `DIRT_SERVER_TOKEN` | Surface to user; rotation required |
| 400 | `BAD_REQUEST` | Malformed JSON, invalid id, negative timestamp, etc. | Don't retry — fix the request |
| 400 | `BATCH_TOO_LARGE` | `notes.length > 500` | Split into batches of ≤ 500 and retry |
| 413 | `PAYLOAD_TOO_LARGE` | Body exceeded `PUSH_BODY_LIMIT` (8 MiB) | Split the batch or trim oversized notes |
| 503 | `TURSO_UNREACHABLE` | Server reached but couldn't talk to Turso | Retry with backoff |
| 500 | `INTERNAL` | Unexpected server bug | Retry with backoff; check server logs |

`SCHEMA_VERSION_MISMATCH` is reserved for Phase 2 (when the wire
format gains a version handshake) and never returned in Phase 1.
`RATE_LIMITED` is also reserved for Phase 2.

## Self-host in 15 minutes

You'll need: a Turso account (free tier works), a deployment target
that supports Rust binaries (Vercel's Rust runtime is what we ship to;
Koyeb and Fly.io also work), and a 16+ character random string for
`DIRT_SERVER_TOKEN`.

```bash
# 1. Create the database on Turso.
turso db create dirt-personal --location nrt   # or your nearest region
turso db tokens create dirt-personal --expiration none > dirt.token

# 2. Capture the connection string and the auth token.
export TURSO_DATABASE_URL="$(turso db show dirt-personal --url)"
export TURSO_AUTH_TOKEN="$(cat dirt.token)"

# 3. Generate a server bearer token (32 random bytes, base64url).
openssl rand -base64 32 | tr -d '=' | tr '+/' '-_' > server.token
export DIRT_SERVER_TOKEN="$(cat server.token)"

# 4. Smoke-test locally.
cargo run -p dirt-api
# In another shell:
curl localhost:8080/healthz   # → ok
curl localhost:8080/v1/notes/pull -H "Authorization: Bearer $DIRT_SERVER_TOKEN"

# 5. Deploy. For Vercel:
npm i -g vercel
vercel link
vercel env add DIRT_SERVER_TOKEN production
vercel env add TURSO_DATABASE_URL production
vercel env add TURSO_AUTH_TOKEN production
vercel --prod

# 6. Set the same DIRT_SERVER_TOKEN value as DIRT_CLIENT_TOKEN on each
#    client (desktop env, mobile build, CLI shell). They must match
#    exactly — the server compares them in constant time.
```

The schema bootstrap runs on the first `dirt-api` start; no migration
scripts to run by hand.

## Behavior reference

- **Solo-phase user**: every accepted bearer token resolves to
  `dirt_core::SOLO_USER_ID` (`01932a0c-3f8b-7e4c-8b1d-3a9c2f5e1234`).
  Phase 2 replaces this with a per-user identity derived from session
  tokens.
- **Server-authoritative timestamps**: pull ordering uses
  `server_updated_at` exclusively. A device with a wall clock 30
  minutes off cannot poison the cursor.
- **Idempotency**: pushing the same id twice is safe — the server
  upserts and returns a fresh `server_updated_at_ms`. Client
  `mark_pushed` runs after the response and stamps the new value.
- **Tag merge**: the *server* doesn't have a tags table. Tag
  maintenance is purely client-side: `dirt_core::db::repository`
  re-runs `sync_tags` on `upsert_from_server` for live rows and
  clears `note_tags` on tombstone.
- **No `user_id` in request bodies**: server derives identity from
  the bearer. A client that tries to assert a different user_id is
  ignored.
