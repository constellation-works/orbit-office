"""Source-checkout exec adapter for the real Gateway handlers (no host install)."""
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'gateway' / 'src'))
from orbit_gateway import audit, connections, drafts, envelope, http

if __name__ == '__main__':
    envelope.serve(sys.stdin.read(), os.environ, {
        'connection-put': connections.tool_put, 'connection-list': connections.tool_list,
        'http-get': http.tool_get, 'http-draft': drafts.tool_draft, 'audit-list': audit.tool_list,
    }, sys.stdout)
