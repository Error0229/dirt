#!/usr/bin/env bash
set -euo pipefail

# Managed-mode security E2E verification script.
#
# This intentionally runs focused cross-client scenarios that validate:
# - managed bootstrap contract
# - runtime fallback/outage behavior
# - session-expiry behavior
# - secret redaction invariants

run_test() {
  local crate="$1"
  local test_name="$2"
  echo "==> ${CARGO_BIN} test -p ${crate} ${test_name}"
  "${CARGO_BIN}" test -p "${crate}" "${test_name}"
}

echo "Running managed-mode security E2E checks..."

if command -v cargo >/dev/null 2>&1; then
  CARGO_BIN="cargo"
elif command -v cargo.exe >/dev/null 2>&1; then
  CARGO_BIN="cargo.exe"
else
  echo "cargo is required but was not found in PATH." >&2
  exit 1
fi

# Backend managed bootstrap + redaction invariants.
run_test dirt-api routes::tests::bootstrap_manifest_returns_schema_and_cache_headers
run_test dirt-api routes::tests::bootstrap_manifest_supports_if_none_match
run_test dirt-api config::tests::config_redacts_sensitive_debug_fields

# CLI managed bootstrap fetch + outage handling + redaction.
run_test dirt-cli bootstrap_manifest::tests::fetch_bootstrap_manifest_parses_valid_payload
run_test dirt-cli bootstrap_manifest::tests::fetch_bootstrap_manifest_surfaces_http_failure
run_test dirt-cli auth::tests::session_debug_redacts_tokens

# Desktop managed bootstrap parsing + auth expiry behavior.
run_test dirt-desktop bootstrap_config::tests::parse_manifest_derives_sync_endpoint_when_missing
run_test dirt-desktop services::auth::tests::session_expiry_uses_safety_skew

# Mobile managed bootstrap parsing + sync offline fallback behavior.
run_test dirt-mobile bootstrap_config::tests::parse_manifest_derives_sync_endpoint_when_missing
run_test dirt-mobile data::tests::in_memory_store_sync_is_noop
run_test dirt-mobile config::tests::env_fallback_is_used_when_runtime_is_incomplete

# Repository-wide secret scanning guard.
bash .github/scripts/security-guard.sh

echo "Managed-mode security E2E checks passed."
