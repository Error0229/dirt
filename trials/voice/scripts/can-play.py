"""Asks a headless browser which recording formats it can really play.

Serves scripts/can-play.html and a folder of sample files on a port of this
computer only, opens the page in the browser given (headless: no window),
and prints what the page reports. Nothing leaves the computer.

    python -I scripts/can-play.py "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" data/play
"""
import http.server
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
browser, folder = sys.argv[1], Path(sys.argv[2]).resolve()
report = threading.Event()
text = []


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(folder), **kwargs)

    def do_GET(self):
        if self.path == "/can-play.html":
            body = (HERE / "can-play.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            super().do_GET()

    def do_POST(self):
        text.append(self.rfile.read(int(self.headers["Content-Length"])).decode())
        self.send_response(204)
        self.end_headers()
        report.set()

    def log_message(self, format, *args):
        if "--verbose" in sys.argv:
            print("  server:", format % args, file=sys.stderr)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
url = f"http://127.0.0.1:{server.server_address[1]}/can-play.html"
with tempfile.TemporaryDirectory() as profile:
    process = subprocess.Popen(
        [browser, "--headless=new", "--disable-gpu", "--no-first-run", "--mute-audio",
         "--autoplay-policy=no-user-gesture-required", f"--user-data-dir={profile}", url],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ok = report.wait(120)
    process.kill()
    process.wait()
print(text[0] if ok else "the page did not report in time")
