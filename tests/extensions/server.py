# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Test site for the sample extensions.

Serves ./site on http://127.0.0.1:8765/ and appends every JSON body POSTed to /report as one
line to reports.jsonl (next to this file), so a test run can be checked afterwards. A second
argument serves another folder (the autofill test pages use this).

    python tests/extensions/server.py
    python tests/extensions/server.py 8766 tests/autofill
"""

import http.server
import json
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
SITE = pathlib.Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else HERE / "site"
REPORTS = HERE / "reports.jsonl"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(SITE), **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        super().end_headers()

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "POST")
        self.send_header("Access-Control-Allow-Headers", "content-type")
        self.end_headers()

    def do_POST(self):
        if self.path != "/report":
            self.send_error(404)
            return
        length = min(int(self.headers.get("Content-Length", "0")), 1_000_000)
        body = self.rfile.read(length).decode("utf-8", "replace")
        try:
            data = json.loads(body)
        except ValueError:
            data = {"raw": body}
        line = json.dumps({"t": round(time.time(), 3), **(data if isinstance(data, dict) else {"data": data})})
        with REPORTS.open("a", encoding="utf-8") as f:
            f.write(line + "\n")
        print(line, flush=True)
        self.send_response(204)
        self.end_headers()


if __name__ == "__main__":
    REPORTS.write_text("", encoding="utf-8")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"serving {SITE} on http://127.0.0.1:{PORT}/", flush=True)
    server.serve_forever()
