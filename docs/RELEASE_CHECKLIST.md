# Release Checklist

Last updated: 2026-02-11

Use this checklist before merging into any `release/*` branch.

## Required Gates

- [ ] Standard CI (`CI` workflow) is fully green.
- [ ] Managed mode security E2E workflow is green for the release candidate commit.
- [ ] Security guardrails pass and no secret-leak findings are present.
- [ ] Open security findings from PR reviews are resolved.

## Managed Mode Security Gate

The `Managed Mode E2E` workflow is mandatory before merge to `release/*`.

Minimum checks covered:
- Bootstrap contract validation.
- Session-expiry and outage handling checks.
- Offline/online sync fallback checks.
- Secret redaction invariants across backend/desktop/mobile/CLI.

## Manual QA Requirement

- [ ] Execute `docs/MANUAL_QA_MANAGED_MODE.md` against release candidate builds.
- [ ] Record pass/fail notes for desktop/mobile/CLI parity scenarios.
- [ ] Any failure requires fix + revalidation before merge.
