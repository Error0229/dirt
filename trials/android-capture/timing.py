"""Trial 2: time from `am start` to a field that can be typed in.

Usage: python timing.py <sheet|sheet-own|app> <cold|warm> <runs>

For each run it clears the log, starts the activity with `am start -W`,
waits, and reads from logcat:
  - ActivityTaskManager's START line (the system received the start)
  - ActivityTaskManager's "Displayed" line (the system's own launch time)
  - the sheet's own lines (first frame, keyboard visible), or the app's
    "TRIAL textarea mounted" (the page says the editor's field exists)
All differences are between logcat timestamps on the emulator's clock.
"""
import re
import statistics
import subprocess
import sys
import time

DEV = "emulator-5554"
PKG = "com.example.DirtMobile"
TARGETS = {
    "sheet": ["-n", f"{PKG}/dev.dirt.capture.CaptureActivity"],
    "sheet-own": ["-n", f"{PKG}/dev.dirt.capture.CaptureActivityOwn"],
    "app": ["-a", "dev.dioxus.main.action.QUICK_CAPTURE", "-n", f"{PKG}/dev.dioxus.main.MainActivity"],
    "app-plain": ["-n", f"{PKG}/dev.dioxus.main.MainActivity"],
}


def adb(*args, timeout=60):
    return subprocess.run(["adb", "-s", DEV, *args], capture_output=True, text=True, timeout=timeout,
                          encoding="utf-8", errors="replace").stdout


def stamp(line):
    m = re.match(r"\d\d-\d\d (\d\d):(\d\d):(\d\d)\.(\d\d\d)", line)
    if not m:
        return None
    h, mi, s, ms = map(int, m.groups())
    return ((h * 60 + mi) * 60 + s) * 1000 + ms


def one(kind, mode, settle):
    if mode == "cold":
        adb("shell", "am", "force-stop", PKG)
        time.sleep(1.5)
    else:
        # Leave whatever is in front: the sheet closes itself when left,
        # the app goes to the background with its process alive.
        adb("shell", "input", "keyevent", "3")
        time.sleep(2.5)
    adb("logcat", "-c")
    out = adb("shell", "am", "start", "-W", *TARGETS[kind])
    total = re.search(r"TotalTime: (\d+)", out)
    wait = re.search(r"WaitTime: (\d+)", out)
    state = re.search(r"LaunchState: (\w+)", out)
    time.sleep(settle)
    log = adb("logcat", "-d", "-v", "threadtime")
    start = shown = frame = keyboard = mounted = None
    proc = None
    for line in log.splitlines():
        t = stamp(line)
        if t is None:
            continue
        if start is None and "ActivityTaskManager" in line and "START u0" in line and PKG in line:
            start = t
        elif "Start proc" in line and PKG in line and proc is None:
            proc = t
        elif "Displayed" in line and PKG in line and shown is None:
            shown = t
        elif "sheet first frame" in line and frame is None:
            frame = t
        elif "sheet keyboard visible" in line and keyboard is None:
            keyboard = t
        elif "TRIAL textarea mounted" in line and mounted is None:
            mounted = t
    rel = lambda t: None if t is None or start is None else t - start
    return {
        "am_total": int(total.group(1)) if total else None,
        "am_wait": int(wait.group(1)) if wait else None,
        "state": state.group(1) if state else "?",
        "proc": rel(proc),
        "displayed": rel(shown),
        "first_frame": rel(frame),
        "keyboard": rel(keyboard),
        "textarea": rel(mounted),
    }


def main():
    kind, mode, runs = sys.argv[1], sys.argv[2], int(sys.argv[3])
    settle = int(__import__("os").environ.get("SETTLE", 9 if kind.startswith("app") else 3))
    rows = []
    if mode == "warm":
        # Bring the process up once first.
        adb("shell", "am", "start", "-W", *TARGETS[kind])
        time.sleep(settle)
    for _ in range(runs):
        row = one(kind, mode, settle)
        rows.append(row)
        print(row, flush=True)
    print(f"--- {kind} {mode}: median of {runs}")
    for key in ["am_total", "am_wait", "proc", "displayed", "first_frame", "keyboard", "textarea"]:
        values = [row[key] for row in rows if row[key] is not None]
        if values:
            print(f"{key:12} median {statistics.median(values):7.0f}  min {min(values):5}  max {max(values):5}  n={len(values)}")
    adb("shell", "input", "keyevent", "3")


if __name__ == "__main__":
    main()
