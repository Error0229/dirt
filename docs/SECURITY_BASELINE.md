# Dirt Security Baseline

Last updated: 2026-02-11 (ops playbook aligned)

This document defines minimum security requirements for authentication, sync, and media flows.

## Security Principles

- Client apps are untrusted environments.
- Long-lived infrastructure credentials must remain server-side only.
- Tokens returned to clients must be short-lived and scoped.
- Sensitive values must never be emitted to logs.

## Credential Classification

- Public client bootstrap config (safe to ship in app):
  - `SUPABASE_URL`
  - `SUPABASE_ANON_KEY`
  - backend API base URL / token exchange endpoint URL
- Server-only secrets (never ship to clients):
  - Turso long-lived auth tokens
  - Cloudflare R2 access key + secret key
  - Supabase service role key

## Threat Model (Minimum)

- Reverse engineering of desktop/mobile binaries.
- Runtime inspection on rooted/jailbroken devices.
- Leaked logs in CI, crash reports, or local diagnostics.
- Replay or abuse of token broker/media signing APIs.

## Required Controls

- Backend must verify Supabase JWT on protected API routes.
- Backend must mint short-lived Turso tokens and presigned media URLs.
- Backend bootstrap manifest must expose only public values and support cache/version semantics.
- Backend must enforce expiration and least privilege on issued credentials.
- Clients must store runtime secrets in OS-provided secure storage.
- Clients must reject malformed bootstrap manifests (unknown fields, unsupported schema versions).
- Sync/media flows must work without user-managed environment variables.

## Logging and Redaction Policy

- Never log raw access tokens, refresh tokens, auth tokens, API keys, or secret keys.
- `Debug` output for security-sensitive structs must redact secret fields.
- Endpoint diagnostics must mask query parameters and credentials.

## Rotation and Revocation

- Rotate server-side Turso/R2 credentials on a fixed schedule.
- Rotate immediately on suspected leak or compromise.
- Support forced session revocation (sign-out all sessions) through auth provider controls.
- Follow `docs/SECURITY_OPERATIONS.md` for inventory, cadence, and incident runbooks.

## Incident Response (Minimum)

1. Revoke and rotate leaked credentials immediately.
2. Disable affected token/signing endpoints until mitigation is in place.
3. Audit recent access logs for misuse windows and impacted users.
4. Ship patched client/backend versions and publish remediation notes.
5. Backfill tests/guardrails to prevent recurrence.

## CI Guardrails

- Security CI must fail on likely secret literals in source.
- Security CI must fail on tracing calls that interpolate secret-like variables.
- Unit tests must verify `Debug` redaction for security-sensitive structs.
