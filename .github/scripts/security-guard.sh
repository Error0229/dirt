#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "Running security guardrails..."

# Guardrail 1: block obvious secret literals from source/workflow files:
# AWS-style access keys, signed tokens in the three-part JWT shape (a Clerk
# ID token is one), and `sk-`-prefixed API keys.
if rg --line-number --pcre2 \
  "AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,}|eyJ[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}" \
  crates .github docs vercel.json \
  -g '!target/**'; then
  echo "ERROR: potential secret literal detected in repository files."
  exit 1
fi

# Guardrail 2: block tracing macros that interpolate secret-like variable names.
if rg --line-number --pcre2 \
  "tracing::(?:trace|debug|info|warn|error)!\\([^\\n)]*\\{[^}]*\\b(?:token|secret|password|access_key|refresh_token|auth_token|client_token|server_token|device_token|id_token)\\b[^}]*\\}[^\\n)]*\\)" \
  crates \
  -g '*.rs'; then
  echo "ERROR: tracing call appears to interpolate a secret-like variable."
  exit 1
fi

# Guardrail 3: block server-only identifiers from the client crates. The
# database connection strings and a Clerk secret key belong to the server's
# environment alone; a client that names one is about to leak it.
CLIENT_CRATES=(
  crates/dirt-core
  crates/dirt-cli
  crates/dirt-desktop
  crates/dirt-mobile
)
if rg --line-number --pcre2 \
  "\\b(?:DATABASE_URL|MIGRATE_DATABASE_URL|CLERK_SECRET_KEY)\\b" \
  "${CLIENT_CRATES[@]}" \
  -g '*.rs'; then
  echo "ERROR: server-only secret identifier referenced in a client crate."
  exit 1
fi

echo "Security guardrails passed."
