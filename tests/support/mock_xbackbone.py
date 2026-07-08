#!/usr/bin/env python3
"""Mock XBackBone HTTP server for ApexShot integration tests.

Mimics the two incompatible XBackBone API versions so we can exercise
ApexShot's real upload/test_connection code paths over HTTP without a
real PHP instance.

Usage:
    python3 mock_xbackbone.py <mode>

Modes:
    v4            4.x instance: POST /api/v1/upload succeeds (Bearer auth, `file` field).
    v3            3.x instance: /api/v1/upload 404s, POST /upload succeeds (`token` field).
    v4_bad_token  4.x instance that rejects the bearer token with 401.
    v3_bad_token  3.x instance that rejects the token field with 404 {message:"Token not found."}.
    v4_quota      4.x instance that returns 413 (quota exceeded).
    v3_quota      3.x instance that returns 507 (disk quota exceeded).

The server binds to 127.0.0.1 on an ephemeral port and prints the chosen
port as the first line of stdout (flushed), so the test harness can read it.
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

VALID_TOKEN = "good-token"


def make_handler(mode: str, base_url: str):
    class Handler(BaseHTTPRequestHandler):
        def _send(self, code: int, obj: dict, content_type: str = "application/json"):
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _read_body(self) -> bytes:
            length = int(self.headers.get("Content-Length", "0") or "0")
            return self.rfile.read(length) if length else b""

        def _log(self, note: str):
            sys.stderr.write(
                f"[{mode}] {self.command} {self.path} -> {note}\n"
            )
            sys.stderr.flush()

        def _validate_multipart(self, body: bytes, expect_field: str, expect_filename: bool):
            """Light validation that ApexShot's hand-built multipart is well-formed."""
            ct = self.headers.get("Content-Type", "")
            if "boundary=" not in ct:
                return "missing boundary in Content-Type"
            boundary = ct.split("boundary=", 1)[1].strip()
            if boundary.encode() not in body:
                return "boundary not present in body"
            if f'name="{expect_field}"'.encode() not in body:
                return f'expected field "{expect_field}" not in body'
            if expect_filename and b"filename=" not in body:
                return "expected filename in body"
            if not body.rstrip().endswith((boundary + "--").encode()):
                return "missing closing boundary"
            return None

        def log_message(self, fmt, *args):  # silence default logging
            pass

        def do_POST(self):  # noqa: N802
            body = self._read_body()

            # --- 4.x endpoint ---
            if self.path == "/api/v1/upload":
                auth = self.headers.get("Authorization", "")
                if mode == "v4_bad_token" and auth != f"Bearer {VALID_TOKEN}":
                    self._log("401 bad token")
                    self._send(401, {"message": "Unauthenticated."})
                    return
                if mode in ("v3", "v3_bad_token", "v3_quota"):
                    # 3.x instances do not have this route.
                    self._log("404 no v4 route (3.x instance)")
                    self._send(404, {"message": "Not Found"})
                    return
                if mode == "v4_quota":
                    self._log("413 quota")
                    self._send(413, {"message": "Quota exceeded."})
                    return
                # v4 success
                err = self._validate_multipart(body, "file", expect_filename=True)
                if err:
                    self._log(f"422 bad multipart: {err}")
                    self._send(422, {"message": f"The file field is required. ({err})"})
                    return
                self._log("200 v4 upload ok")
                self._send(
                    200,
                    {
                        "data": {
                            "id": 1,
                            "preview_ext_url": f"{base_url}/p/abc",
                            "raw_url": f"{base_url}/r/abc.png",
                            "deletion_url": f"{base_url}/d/abc",
                        }
                    },
                )
                return

            # --- 3.x endpoint ---
            if self.path == "/upload":
                if mode in ("v4", "v4_bad_token", "v4_quota"):
                    # 4.x instances do not have this route.
                    self._log("404 no v3 route (4.x instance)")
                    self._send(404, {"message": "Not Found"})
                    return
                if mode == "v3_bad_token":
                    self._log("404 token rejected")
                    self._send(404, {"message": "Token not found.", "version": "3.8.2"})
                    return
                if mode == "v3_quota":
                    self._log("507 disk quota")
                    self._send(507, {"message": "User disk quota exceeded.", "version": "3.8.2"})
                    return
                # v3 success
                err = self._validate_multipart(body, "upload", expect_filename=True)
                if err and body.strip():
                    # A non-empty body that fails validation is a real error.
                    self._log(f"400 bad multipart: {err}")
                    self._send(400, {"message": "Bad request.", "version": "3.8.2"})
                    return
                if not body.strip() or b"filename=" not in body:
                    # test_connection_v3 probe: empty/empty-file body.
                    self._log("400 no file attached (probe accepted)")
                    self._send(400, {"message": "Request without file attached.", "version": "3.8.2"})
                    return
                self._log("201 v3 upload ok")
                self._send(
                    201,
                    {
                        "message": "OK",
                        "version": "3.8.2",
                        "url": f"{base_url}/abc.png",
                        "raw_url": f"{base_url}/raw/abc.png",
                    },
                )
                return

            self._log("404 unknown path")
            self._send(404, {"message": "Not Found"})

        def do_GET(self):  # noqa: N802
            self._log("404 GET not handled")
            self._send(404, {"message": "Not Found"})

    return Handler


def main():
    if len(sys.argv) < 2:
        sys.stderr.write("usage: mock_xbackbone.py <mode>\n")
        sys.exit(2)
    mode = sys.argv[1]
    server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(mode, "http://xb.test"))
    port = server.server_address[1]
    # First line of stdout is the port; the test harness reads it.
    sys.stdout.write(f"{port}\n")
    sys.stdout.flush()
    sys.stderr.write(f"[{mode}] listening on 127.0.0.1:{port}\n")
    sys.stderr.flush()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
