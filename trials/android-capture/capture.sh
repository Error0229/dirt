#!/usr/bin/env bash
# capture.sh <activity: CaptureActivity|CaptureActivityOwn> "<words>" [extra am start args...]
# Opens the sheet, types the words one at a time, presses the tick.
. "$(dirname "$0")/env.sh"
act="$1"; words="$2"; shift 2
a shell am start -W -n "$PKG/dev.dirt.capture.$act" "$@" | grep -E "LaunchState|TotalTime|Error" | tr '\n' ' '; echo
sleep 2
first=1
for word in $words; do
  [ $first = 1 ] || a shell input keyevent 62
  first=0
  a shell input text "$word"
  sleep 0.3
done
sleep 0.7
python "$S/ui.py" | grep -E "EditText" | head -1
python "$S/ui.py" tap "Keep"
