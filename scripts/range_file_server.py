#!/usr/bin/env python3
"""A loopback HTTP file server with Range support and a request log.

The model store's probes (`scripts/jobs-probe/model_store.swift`) download through the app's
own `ModelDownloader` from this server instead of the network. It serves files under `root`
read at request time, answers `Range: bytes=<start>-` / `bytes=<start>-<end>` with 206 and a
`Content-Range`, a start past the end with 416, and everything else with 200. Every request is
appended to `log` as one JSON line: `{"method", "path", "range", "status", "bytes"}`.

Standard library only. `start(root, log)` runs it on 127.0.0.1 at a free port in a daemon thread
and returns `(server, base_url)`; run as a script it serves until interrupted.
"""

import argparse
import http.server
import json
import threading
from pathlib import Path


def _handler(root: Path, log: Path):
    lock = threading.Lock()

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):  # the JSON log replaces stderr lines
            pass

        def _record(self, status: int, sent: int) -> None:
            entry = {"method": self.command, "path": self.path, "range": self.headers.get("Range"),
                     "status": status, "bytes": sent}
            with lock, log.open("a") as stream:
                stream.write(json.dumps(entry) + "\n")

        def _answer(self, status: int, body: bytes, headers: dict[str, str], send_body: bool) -> None:
            self.send_response(status)
            for name, value in headers.items():
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if send_body:
                self.wfile.write(body)
            self._record(status, len(body) if send_body else 0)

        def _serve(self, send_body: bool) -> None:
            relative = self.path.split("?", 1)[0].lstrip("/")
            path = (root / relative).resolve()
            if root.resolve() not in path.parents or not path.is_file():
                self._answer(404, b"not found", {"Content-Type": "text/plain"}, send_body)
                return
            data = path.read_bytes()
            wanted = self.headers.get("Range")
            if wanted is None or not wanted.startswith("bytes="):
                self._answer(200, data, {"Content-Type": "application/octet-stream",
                                         "Accept-Ranges": "bytes"}, send_body)
                return
            first, _, last = wanted[len("bytes="):].partition("-")
            try:
                start = int(first)
                end = int(last) if last else len(data) - 1
            except ValueError:
                self._answer(400, b"bad range", {"Content-Type": "text/plain"}, send_body)
                return
            if start >= len(data):
                self._answer(416, b"", {"Content-Range": f"bytes */{len(data)}"}, send_body)
                return
            end = min(end, len(data) - 1)
            self._answer(206, data[start:end + 1], {
                "Content-Type": "application/octet-stream", "Accept-Ranges": "bytes",
                "Content-Range": f"bytes {start}-{end}/{len(data)}"}, send_body)

        def do_GET(self):  # noqa: N802 (http.server's name)
            self._serve(send_body=True)

        def do_HEAD(self):  # noqa: N802
            self._serve(send_body=False)

    return Handler


def start(root: Path, log: Path):
    """Serves `root` on 127.0.0.1 at a free port in a daemon thread: `(server, base_url)`."""
    root.mkdir(parents=True, exist_ok=True)
    log.touch()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _handler(root, log))
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}/"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("log", type=Path)
    arguments = parser.parse_args()
    server, url = start(arguments.root, arguments.log)
    print(f"serving {arguments.root} at {url}", flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        server.shutdown()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
