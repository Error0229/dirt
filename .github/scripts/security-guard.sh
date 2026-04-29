#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "Running security guardrails..."

# Guardrail 1: block obvious secret literals from source/workflow files.
# Patterns we still care about post-Supabase: AWS access keys (Turso
# platform tokens follow a similar shape), generic JWT triplets (the
# TURSO_AUTH_TOKEN format), and `sk-`-prefixed API keys (OpenAI etc).
if rg --line-number --pcre2 \
  "AKIA[0-9A-Z]{16}|sk-[A-Za-z0-9]{20,}|eyJ[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]{20,}" \
  crates .github docs vercel.json \
  -g '!target/**'; then
  echo "ERROR: potential secret literal detected in repository files."
  exit 1
fi

# Guardrail 2: block tracing macros that interpolate secret-like variable names.
if rg --line-number --pcre2 \
  "tracing::(?:trace|debug|info|warn|error)!\\([^\\n)]*\\{[^}]*\\b(?:token|secret|password|access_key|refresh_token|auth_token|client_token|server_token|turso_auth_token|dirt_server_token|dirt_client_token)\\b[^}]*\\}[^\\n)]*\\)" \
  crates \
  -g '*.rs'; then
  echo "ERROR: tracing call appears to interpolate a secret-like variable."
  exit 1
fi

# Guardrail 3: block server-only secret identifiers from client crates.
# DIRT_SERVER_TOKEN is the post-Supabase shared bearer token: client
# crates must never reference it; only `DIRT_CLIENT_TOKEN` belongs on
# the client side. TURSO_AUTH_TOKEN is the database-scoped admin token
# the server holds; it must never reach a client.
CLIENT_CRATES=(
  crates/dirt-cli
  crates/dirt-desktop
  crates/dirt-mobile
)
if rg --line-number --pcre2 \
  "\\b(?:DIRT_SERVER_TOKEN|TURSO_AUTH_TOKEN|TURSO_DATABASE_URL|TURSO_ADMIN_AUTH_TOKEN|TURSO_GROUP_TOKEN|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY)\\b" \
  "${CLIENT_CRATES[@]}" \
  -g '*.rs'; then
  echo "ERROR: server-only secret identifier referenced in a client crate."
  exit 1
fi

echo "Security guardrails passed."
