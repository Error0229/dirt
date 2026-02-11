# Dirt Security Operations

Last updated: 2026-02-11

This document defines operational controls for managed credentials and backend secret handling.

## Scope

- Applies to managed auth, sync token exchange, and media signing flows.
- Covers backend secret inventory, rotation, revocation, and incident response.
- Complements `docs/SECURITY_BASELINE.md`.

## Secret Inventory

| Secret | Purpose | Storage | Rotation cadence | Owner |
|--------|---------|---------|------------------|-------|
| `TURSO_DATABASE_URL` + server auth token | Backend sync token minting | Backend secret store / runtime env | Every 90 days (or immediately on incident) | Backend |
| `R2_ACCESS_KEY_ID` + `R2_SECRET_ACCESS_KEY` | Media signing and object access | Backend secret store / runtime env | Every 90 days (or immediately on incident) | Backend |
| `SUPABASE_SERVICE_ROLE_KEY` | Admin/privileged Supabase operations | Backend secret store / runtime env | Every 60 days (or immediately on incident) | Backend |

Client apps must never embed, persist, or log the secrets above.

## Least-Privilege Runtime Policy

- Backend deploy identity can read only the secret keys listed in the inventory.
- Backend runtime cannot write/rotate secrets directly; rotation happens through infra admin path.
- CI jobs use scoped tokens and do not receive production secret material.
- Non-production environments use isolated secret sets (no secret sharing between environments).

## Rotation Playbook

1. Create replacement credentials in provider console/API (Turso/R2/Supabase).
2. Store new values in backend secret store for target environment.
3. Deploy backend with dual-read compatibility where supported.
4. Verify health checks and token/signing API success rates.
5. Revoke old credentials.
6. Confirm no error-rate spike after revocation.
7. Record rotation timestamp, operator, and change ticket.

## Revocation and Incident Response

If compromise is suspected:

1. Disable affected token/signing endpoints if abuse is active.
2. Revoke compromised credentials immediately.
3. Rotate credentials and redeploy backend.
4. Force session revocation where applicable.
5. Review access logs and identify impact window.
6. Publish remediation summary and follow-up actions.

Quarterly drill requirement:

- Run one tabletop simulation covering credential leakage and endpoint abuse.
- Capture response duration and any procedural gaps.

## Monitoring and Alerts

Minimum backend alerts:

- Sudden spike in `POST /v1/sync/token` failures or rate-limit hits.
- Sudden spike in media presign failures.
- Excessive token issuance per user/device/IP window.
- Any log event indicating secret-redaction failure.

Operational metrics to keep:

- Token issuance success/failure ratio.
- Signing success/failure ratio.
- 401/403/429 rates by endpoint.
- P95/P99 latency for token/signing endpoints.

## Release Security Gates

- Security guard CI passes (including client secret-identifier scan).
- Secret redaction tests pass.
- Managed mode security E2E workflow passes for release candidate commit.
- `docs/RELEASE_CHECKLIST.md` is completed before merge to `release/*`.
- Latest rotation/revocation runbook remains valid and reviewed.
