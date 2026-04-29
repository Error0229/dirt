# Dirt Security Baseline

This document defines minimum security requirements for the
`dirt-api` sync backend and its clients. Phase 1 is a single-user
deployment using a shared bearer token; Phase 2's magic-link auth will
extend (not replace) these baselines.

## Security Principles

- Client binaries are untrusted environments. Anything baked into a
  shipped client is effectively public.
- Long-lived infrastructure credentials are server-only and never
  reach clients.
- Sensitive values never appear in logs, panic output, or `Debug`
  prints. The `Debug` impls for `AppConfig`, `ServerToken`, and
  `ApiClient` redact tokens; new credential-bearing types must follow
  the same pattern.

## Credential Classification

**Server-only (never bake into a client):**
- `DIRT_SERVER_TOKEN` — shared bearer that clients must present.
- `TURSO_DATABASE_URL`, `TURSO_AUTH_TOKEN` — long-lived
  database-scoped credentials. The auth token is a JWT-shaped string
  generated via `turso db tokens create`.

**Client-side runtime env (read at process start, never baked):**
- `DIRT_CLIENT_TOKEN` — must equal `DIRT_SERVER_TOKEN`. Compared on
  the server with `subtle::ConstantTimeEq`.
- `DIRT_API_BASE_URL` — public URL of the deployed `dirt-api`.

**Build-baked (safe to ship inside a client binary):**
- `DIRT_API_BASE_URL` only, when distributing pre-configured builds.
  The build-baked value can always be overridden by the runtime env
  var; clients must not require the runtime override to function.

## Threat Model

- Reverse engineering of desktop/mobile binaries: clients must not
  contain `DIRT_SERVER_TOKEN` or `TURSO_AUTH_TOKEN`.
- Runtime inspection (rooted/jailbroken devices): bearer-token
  exfiltration leaks the entire dataset. Mitigation: server-side
  rotation + per-user identities in Phase 2.
- Leaked logs in CI, crash reports, or local diagnostics: token
  redaction in every `Debug` impl + tracing-macro guardrail in CI.
- Body-size DoS against `/v1/notes/push`: 8 MiB body limit
  (`PUSH_BODY_LIMIT`) short-circuits oversized requests before they
  reach the handler.
- Token replay across networks: Phase 1 has no rate limiter (deferred
  to Phase 2 because IP-based limits self-DoS a single-user deploy
  with desktop + phone + CLI on one LAN).

## Required Controls

- Server middleware verifies `Authorization: Bearer <token>` against
  `DIRT_SERVER_TOKEN` using `subtle::ConstantTimeEq`. Length check
  short-circuits before the constant-time compare so attacker-supplied
  length doesn't widen the timing surface.
- `/v1/notes/push` has a request body cap (`DefaultBodyLimit` set to
  8 MiB).
- `/v1/notes/pull` orders by **server-authoritative** timestamps; a
  misbehaving client clock cannot poison the cursor.
- `user_id` is derived from the bearer token; client-asserted
  `user_id` in request bodies is rejected.
- Client `Debug` output for credential-bearing structs prints
  `[REDACTED]` instead of the token.
- Misconfigured env vars surface as visible `SyncStatus::Error`
  instead of silently disabling sync.

## Logging and Redaction Policy

- Never log raw bearer tokens, Turso auth tokens, or `Authorization`
  headers.
- `Debug` impls for `AppConfig`, `ServerToken`, and `ApiClient`
  replace token fields with `[REDACTED]`. New types holding tokens
  must do the same.
- Tracing macros that interpolate variables named like secrets
  (`token`, `secret`, `auth_token`, `client_token`,
  `server_updated_at` — wait, that last one's fine; the regex is in
  `.github/scripts/security-guard.sh`) fail CI.

## Rotation and Revocation

- Rotate `DIRT_SERVER_TOKEN` on suspected leak: generate a new
  token, deploy the new value to the server, then update every
  client's `DIRT_CLIENT_TOKEN`. Old token stops working as soon as
  the server picks up the new value.
- Rotate `TURSO_AUTH_TOKEN` on suspected leak via
  `turso db tokens revoke <db>` followed by `tokens create`.
- There is no per-user revocation in Phase 1 (no per-user identity).
  Magic-link auth in Phase 2 adds session-token revocation.

See [docs/SECURITY_OPERATIONS.md](./SECURITY_OPERATIONS.md) for
cadence and incident runbooks.

## Incident Response (Minimum)

1. Revoke and rotate leaked credentials immediately.
2. Audit Turso access logs for misuse windows.
3. If `DIRT_SERVER_TOKEN` leaked: rotate server-side, push new
   `DIRT_CLIENT_TOKEN` to every client.
4. If `TURSO_AUTH_TOKEN` leaked: rotate, redeploy `dirt-api` with
   the new env, force-disconnect any sessions if Turso provides that
   knob.
5. Backfill tests/guardrails to prevent recurrence (e.g. a new
   pattern in `security-guard.sh`).

## CI Guardrails

`bash .github/scripts/security-guard.sh` runs in CI and locally:

- Blocks secret-shaped literals in source (`AKIA...`, `sk-...`, JWT
  triplets).
- Blocks tracing macros that interpolate secret-like variable names.
- Blocks server-only env-var names (`DIRT_SERVER_TOKEN`,
  `TURSO_AUTH_TOKEN`, `TURSO_DATABASE_URL`,
  `TURSO_ADMIN_AUTH_TOKEN`, `TURSO_GROUP_TOKEN`, `AWS_*`) from
  appearing inside any client crate.

Unit tests verify `Debug` redaction for `ServerToken` and
`ApiClient`. New token-bearing types must extend that test set.
