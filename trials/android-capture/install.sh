#!/usr/bin/env bash
. "$(dirname "$0")/env.sh"
set -e
a push "$GEN/app/build/outputs/apk/debug/app-debug.apk" /data/local/tmp/dirt.apk | tail -1
a shell pm install -r /data/local/tmp/dirt.apk
