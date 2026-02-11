# Managed Mode Manual QA Guide

Last updated: 2026-02-11

This guide verifies managed-mode parity for desktop, mobile, and CLI.

## Scope

- Fresh install bootstrap without user-provided infra keys.
- Auth/session lifecycle: login, logout, re-login, expiry recovery.
- Sync and attachment operations.
- Offline/online transitions and backend outage behavior.
- Secret-redaction checks in logs/diagnostics.

## Preconditions

- Backend API is deployed and `/v1/bootstrap` is reachable.
- Supabase email/password auth is configured.
- Turso sync broker and optional media signing are configured on backend.
- Test account exists (or sign-up flow enabled in test environment).

## Parity Matrix

| Scenario | Desktop | Mobile | CLI | Expected result |
|---|---|---|---|---|
| Fresh install/start | Launch app | Install APK and launch | Run `dirt config init --bootstrap-url <url>` | Managed config resolves without entering Turso/R2 secrets |
| Login | Sign in from Settings | Sign in from Settings | `dirt auth login --email ... --password ...` | Session established |
| Sync | Create note and wait scheduler | Create note and wait scheduler | `dirt sync` | Note appears on all clients |
| Attachment upload/open | Upload + open in-app modal | Upload + open preview | N/A (metadata parity only) | Attachment roundtrip succeeds |
| Logout/re-login | Sign out then sign in | Sign out then sign in | `dirt auth logout` then login | Session clears then re-establishes |

## Offline/Online Transition

1. Sign in and confirm managed sync is healthy.
2. Disable network.
3. Create/update notes on each client.
4. Re-enable network.
5. Verify pending changes sync successfully and conflicts are handled.

Expected:
- Writes continue offline.
- Sync recovers automatically when network returns.

## Expired Session and Backend Outage

1. Force an expired session (or wait expiry).
2. Attempt sync and attachment actions.
3. Verify refresh/re-auth prompts are shown.
4. Simulate backend outage (e.g., block API endpoint).
5. Attempt managed sync/media actions.

Expected:
- User-facing failure is actionable and non-technical.
- App remains usable for local/offline notes.
- Recovery works after backend is restored.

## Secret Leakage Checks

- Inspect client logs and diagnostics screens.
- Inspect backend logs for token/signing endpoints.

Must never appear:
- Supabase access/refresh tokens.
- Turso auth tokens.
- R2 secret keys.
- Any server-only credential values.

## Pass/Fail Criteria

Pass when:
- All matrix scenarios succeed for desktop/mobile/CLI.
- Offline and outage flows match expected behavior.
- No secret leakage is observed in logs/diagnostics.

Fail when any required scenario regresses or any secret value is exposed.
