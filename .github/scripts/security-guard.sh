#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "Running security guardrails..."

# Guardrail 1: block obvious secret literals from source/workflow files.
if rg --line-number --pcre2 \
  "AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,}|eyJ[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}" \
  crates .github \
  -g '!target/**'; then
  echo "ERROR: potential secret literal detected in repository files."
  exit 1
fi

# Guardrail 2: block tracing macros that interpolate secret-like variable names.
if rg --line-number --pcre2 \
  "tracing::(?:trace|debug|info|warn|error)!\\([^\\n)]*\\{[^}]*\\b(?:token|secret|password|access_key|refresh_token|auth_token)\\b[^}]*\\}[^\\n)]*\\)" \
  crates \
  -g '*.rs'; then
  echo "ERROR: tracing call appears to interpolate a secret-like variable."
  exit 1
fi

# Guardrail 3: block server-only secret identifiers from client crates.
CLIENT_CRATES=(
  crates/dirt-cli
  crates/dirt-desktop
  crates/dirt-mobile
)
if rg --line-number --pcre2 \
  "\\b(?:SUPABASE_SERVICE_ROLE_KEY|R2_ACCESS_KEY_ID|R2_SECRET_ACCESS_KEY|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|TURSO_ADMIN_AUTH_TOKEN|TURSO_GROUP_TOKEN)\\b" \
  "${CLIENT_CRATES[@]}" \
  -g '*.rs'; then
  echo "ERROR: server-only secret identifier referenced in a client crate."
  exit 1
fi

echo "Security guardrails passed."
