"""Stand-in for the running app's /build-info.json, for test-install.sh.

usage: app-build-info-responder.py <build-info.json> <mode-file> <port-file>

Serves the file the installer publishes, so a restart "reports" whatever the
last publication wrote. When the mode file reads "wrong" it reports a commit
nobody built, as a service that kept running old code would.
"""
import http.server
import pathlib
import sys

info, mode, port_file = (pathlib.Path(arg) for arg in sys.argv[1:4])


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if mode.exists() and mode.read_text().strip() == "wrong":
            data = b'{"commit":"wrong","build":"wrong"}'
        elif info.exists():
            data = info.read_bytes()
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port_file.write_text(str(server.server_port))
server.serve_forever()
