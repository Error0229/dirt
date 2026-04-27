# Dirt Security Operations

Operational controls for `dirt-api` secret handling and incident
response. Pairs with [docs/SECURITY_BASELINE.md](./SECURITY_BASELINE.md).

## Scope

- Applies to the deployed `dirt-api` server, the Turso database it
  fronts, and the bearer token shared between server and clients.
- Covers secret inventory, rotation cadence, and incident runbooks.
- Phase 1 only. Phase 2 (magic-link auth) extends this with
  per-user session-token revocation.

## Secret Inventory

| Secret | Purpose | Storage | Rotation cadence | Owner |
|---|---|---|---|---|
| `DIRT_SERVER_TOKEN` | Shared bearer; server compares against the `Authorization` header | Vercel env (or your deploy target) + every client's runtime env | Every 90 days, or immediately on suspected leak | Backend operator |
| `TURSO_AUTH_TOKEN` | Long-lived database-scoped token for `dirt-api` to read/write the Turso DB | Vercel env + local `.env.server` for dev | Every 180 days, or immediately on suspected leak | Backend operator |
| `TURSO_DATABASE_URL` | Endpoint URL for the Turso database | Vercel env + local `.env.server` | Only changes if the database is migrated or renamed | Backend operator |

Client binaries must never embed, persist, or log
`DIRT_SERVER_TOKEN`, `TURSO_AUTH_TOKEN`, or `TURSO_DATABASE_URL`.
The `security-guard.sh` CI step scans every client crate for these
identifiers.

## Least-Privilege Runtime Policy

- The Vercel deploy identity holds only the three env vars listed
  above. It does not have Turso platform-token permissions.
- The `dirt-api` runtime cannot rotate secrets — rotation happens out
  of band via Turso CLI / Vercel CLI / `openssl rand`.
- CI uses scoped GitHub tokens that cannot read production secret
  values; the security-guard step runs without credentials.
- Non-production deploys (preview, staging) must use a different
  Turso database and a different `DIRT_SERVER_TOKEN`. Sharing tokens
  across environments turns one leak into a multi-environment
  incident.

## Rotation Playbook

### `DIRT_SERVER_TOKEN`

1. Generate a replacement: `openssl rand -base64 32 | tr -d '=' | tr '+/' '-_'`.
2. Update the server env: `vercel env add DIRT_SERVER_TOKEN production`.
3. Promote a deploy that picks up the new env (the value is read at
   process start; restarting / redeploying is required).
4. Update every client's `DIRT_CLIENT_TOKEN` to the new value. Push
   the new env to whichever distribution channel each client uses.
5. Smoke against the deployed URL with the new token; assert old
   token is rejected (`401 UNAUTHORIZED`).
6. Record the rotation timestamp + operator in the ops log.

There is no dual-read window in Phase 1 — the comparison is exactly
one token. Schedule rotations during a maintenance window where
clients can be updated together.

### `TURSO_AUTH_TOKEN`

1. Generate a replacement: `turso db tokens create <db-name>
   --expiration none > new.token`.
2. Update the server env: `vercel env add TURSO_AUTH_TOKEN production`,
   paste the new token value.
3. Redeploy `dirt-api`.
4. Verify with `curl -s "$VERCEL_URL/v1/notes/pull?limit=1" -H
   "Authorization: Bearer $DIRT_SERVER_TOKEN"` — non-zero notes
   means the new token works.
5. Revoke the old token: `turso db tokens revoke <db-name> <old-token-fingerprint>`.
6. Record the rotation.

## Revocation and Incident Response

If a leak is suspected:

1. **Bearer token (`DIRT_SERVER_TOKEN`).** Rotate immediately per
   the playbook above. Anyone holding the old token loses access on
   the next deploy. There is no per-device revocation in Phase 1; a
   leaked token revokes the *whole deployment*.
2. **Turso auth token (`TURSO_AUTH_TOKEN`).** Rotate via the Turso
   playbook. The old token continues to work until you explicitly
   revoke it server-side.
3. **Audit access logs.** Vercel logs include request paths, source
   IPs, and status codes. Look for unfamiliar IPs hitting `/v1/*`
   between the suspected leak time and rotation time.
4. **Snapshot the database.** Even if abuse is unlikely, a snapshot
   makes a comparable rollback target if you later discover
   tampering. See [DEPLOY.md](./DEPLOY.md) for the snapshot recipe.
5. **Publish the remediation note** so operators of other instances
   know to rotate too.
6. **Backfill a CI guardrail.** If the leak was a regex you weren't
   blocking, add it to `.github/scripts/security-guard.sh`.

### Quarterly drill

Once per quarter, run a tabletop simulation covering bearer-token
leakage and Turso credential leakage. Time the rotation end-to-end;
if it takes more than 30 minutes, the playbook needs simplification.

## Monitoring and Alerts

Minimum operational signals (Vercel + Turso dashboards):

- Sudden spike in `401 UNAUTHORIZED` rate on `/v1/notes/*`. Could
  indicate token-rotation lag or a brute-force attempt.
- Sudden spike in `503 TURSO_UNREACHABLE`. Indicates a Turso outage
  or the server lost its credentials.
- `/healthz` failing. Indicates the server is down.
- Function execution time on `/v1/notes/push` exceeding 5 s P95.
  Suggests Turso latency degradation.

## Release Security Gates

Before promoting a deploy to production:

- `cargo test --workspace` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `bash .github/scripts/security-guard.sh` exits clean.
- Pre-deploy snapshot taken; rollback dry-run executed
  (see [DEPLOY.md](./DEPLOY.md#pre-deploy-checklist)).
- A reviewer confirmed `Debug` redaction for any new
  credential-bearing struct.
