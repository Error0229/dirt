#!/usr/bin/env bash
# Signs an APK again with the key in a PKCS12 keystore, and checks what it
# made: signed by that key and no other, its signatures valid, and aligned
# as Android wants (4 bytes, and 16 KiB pages for native libraries).
#
#   sign-apk.sh <keystore> <alias> <apk in> <apk out>
#
# The keystore's password (the key's too) comes from the environment as
# DIRT_KEYSTORE_PASSWORD, and reaches apksigner and keytool from there:
# never on a command line or in a file. BUILD_TOOLS names the Android build
# tools to use. Prints the signing certificate's SHA-256 on standard output,
# and the tools' own words on standard error.
#
# The release workflow runs this, the same commands, in both places that
# sign: every run signs its APK with a stand-in key made for the run, so
# that the commands are tried each time; a tag's publish job signs with the
# release key (docs/RELEASING.md).
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: sign-apk.sh <keystore> <alias> <apk in> <apk out>" >&2
  exit 2
fi
keystore=$1
alias=$2
input=$3
output=$4
: "${DIRT_KEYSTORE_PASSWORD:?DIRT_KEYSTORE_PASSWORD holds no password}"
: "${BUILD_TOOLS:?BUILD_TOOLS does not name the Android build tools}"

"$BUILD_TOOLS/apksigner" sign --ks "$keystore" --ks-type PKCS12 --ks-key-alias "$alias" \
  --ks-pass env:DIRT_KEYSTORE_PASSWORD --key-pass env:DIRT_KEYSTORE_PASSWORD \
  --v4-signing-enabled false \
  --out "$output" "$input" >&2

want=$(keytool -exportcert -keystore "$keystore" -storetype PKCS12 -alias "$alias" \
  -storepass:env DIRT_KEYSTORE_PASSWORD | sha256sum | cut -d' ' -f1)
"$BUILD_TOOLS/apksigner" verify "$output" >&2
certs=$("$BUILD_TOOLS/apksigner" verify --print-certs "$output")
got=$(echo "$certs" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')
signers=$(echo "$certs" | grep -c '^Signer #[0-9]* certificate DN' || true)
if [ -z "$want" ] || [ "$want" != "$got" ] || [ "$signers" != "1" ]; then
  echo "::error::$output is not signed by the key in $keystore alone." >&2
  exit 1
fi
"$BUILD_TOOLS/zipalign" -c -P 16 4 "$output" >&2
echo "$got"
