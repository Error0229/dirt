#!/usr/bin/env bash
# Signs the phone's app in and lets it into the account from the CLI.
cd "$(dirname "$0")"; . ./env.sh
waitfor() { for i in $(seq 1 20); do python ui.py 2>/dev/null | grep -q "$1" && return 0; sleep 2; done; echo "never saw: $1"; python ui.py | cut -c30-150 | head -12; return 1; }
a shell am start -n $PKG/dev.dioxus.main.MainActivity >/dev/null
waitfor "Settings" || exit 1
python ui.py tap "Settings"; waitfor "Sign in" || exit 1
python ui.py tap "Sign in"; waitfor "Let this device in" || exit 1
python ui.py tap "Let this device in"; waitfor "Ask another device" || exit 1
python ui.py tap "Ask another device"; sleep 3
(echo y | ./cli.sh devices approve > cli-approve.log 2>&1 &)
waitfor "They match" || exit 1
python ui.py | grep -E "[0-9]{4} [0-9]{4} [0-9]{4}" | cut -c60-120; grep "Code" cli-approve.log
python ui.py tap "They match"; waitfor "was let in" || exit 1
sleep 3; tail -1 cli-approve.log
./cli.sh devices list 2>&1 | cut -c15-80
a shell input keyevent 3
