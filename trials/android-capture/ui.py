"""Trial 2: look at and press the emulator's screen through uiautomator.

  python ui.py                 list what is on screen (text, description, bounds)
  python ui.py tap "<label>"   tap the first node whose text or description contains it
  python ui.py shot <file>     save a screenshot
"""
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

DEV = "emulator-5554"


def adb(*args, binary=False):
    done = subprocess.run(["adb", "-s", DEV, *args], capture_output=True, timeout=60)
    return done.stdout if binary else done.stdout.decode("utf-8", "replace")


def nodes():
    raw = adb("exec-out", "uiautomator", "dump", "/dev/tty")
    start = raw.find("<?xml")
    end = raw.rfind("</hierarchy>")
    if start < 0 or end < 0:
        return []
    tree = ET.fromstring(raw[start:end + len("</hierarchy>")])
    found = []
    for node in tree.iter("node"):
        text = node.get("text") or ""
        desc = node.get("content-desc") or ""
        m = re.match(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", node.get("bounds") or "")
        if not m:
            continue
        x1, y1, x2, y2 = map(int, m.groups())
        found.append({
            "text": text, "desc": desc, "pkg": node.get("package") or "",
            "cls": (node.get("class") or "").split(".")[-1],
            "x": (x1 + x2) // 2, "y": (y1 + y2) // 2,
            "focused": node.get("focused") == "true",
        })
    return found


def show():
    for n in nodes():
        if n["text"] or n["desc"]:
            mark = " *focused*" if n["focused"] else ""
            print(f'{n["pkg"]:28.28} {n["cls"]:14.14} ({n["x"]:4},{n["y"]:4}) text={n["text"]!r} desc={n["desc"]!r}{mark}')


def main():
    if len(sys.argv) == 1:
        show()
    elif sys.argv[1] == "tap":
        want = sys.argv[2].lower()
        for n in nodes():
            if want in n["text"].lower() or want in n["desc"].lower():
                adb("shell", "input", "tap", str(n["x"]), str(n["y"]))
                print(f'tapped {n["text"] or n["desc"]!r} at {n["x"]},{n["y"]}')
                return
        print("not found:", sys.argv[2])
        show()
        sys.exit(1)
    elif sys.argv[1] == "shot":
        png = adb("exec-out", "screencap", "-p", binary=True)
        open(sys.argv[2], "wb").write(png)
        print("saved", sys.argv[2], len(png))


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    main()
