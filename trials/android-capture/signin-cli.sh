#!/usr/bin/env bash
cd "$(dirname "$0")"
rm -f cli-login.log
(./cli.sh auth login --no-browser > cli-login.log 2>&1 &)
for i in $(seq 1 15); do grep -q "http" cli-login.log 2>/dev/null && break; sleep 1; done
ADDR=$(grep -o "http[^ ]*" cli-login.log | head -1)
curl -sL "${ADDR}&as=alice" | head -c 120; echo
sleep 4
./cli.sh auth status 2>&1 | sed -E 's/[0-9a-f]{8}-[0-9a-f-]{27}/<id>/g' | head -4
