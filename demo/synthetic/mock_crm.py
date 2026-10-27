#!/usr/bin/env python3
"""Loopback Acme CRM. Invented records, deliberately fake bearer token.

GET echoes the token so the real gateway's value redactor can demonstrate
[secret]. PATCH is refused: the seeded write must stay pending for review.
"""
import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading

from common import FAKE_TOKEN


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def valid_origin(self):
        authority = f"127.0.0.1:{self.server.server_port}"
        return (self.headers.get("Host") == authority and
                self.headers.get("Origin", "http://" + authority) == "http://" + authority)

    def answer(self, code, data):
        body = json.dumps(data, sort_keys=True).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not self.valid_origin():
            return self.answer(403, {"error": "loopback authority required"})
        if self.headers.get("Authorization") != "Bearer " + FAKE_TOKEN:
            return self.answer(401, {"error": "fake demo token required"})
        if self.path != "/v1/accounts/acme":
            return self.answer(404, {"error": "unknown simulated record"})
        self.answer(200, {"simulated": True, "account": "Acme Paper Kites",
                          "stage": "pilot", "owner": "Mira Vale",
                          "echo_token": FAKE_TOKEN})

    def do_PATCH(self):
        self.server.patch_count += 1
        self.answer(405, {"error": "SIMULATED: review the draft; no write endpoint"})


@contextmanager
def serving(handler=Handler, port=0):
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    server.patch_count = 0
    server.daemon_threads = False
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        if thread.is_alive():
            raise RuntimeError("mock server did not stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=4329)
    args = parser.parse_args()
    with serving(port=args.port) as instance:
        print(f"SIMULATED Acme CRM on http://127.0.0.1:{instance.server_port}", flush=True)
        try:
            threading.Event().wait()
        except KeyboardInterrupt:
            pass
