#!/usr/bin/env python3
"""Exercise the staged Atrium server/session boundary without a Peios kernel."""

from __future__ import annotations

import argparse
import http.client
import json
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


EXPECTED_APPS = {
    "org.peios.about",
    "org.peios.packages",
    "org.peios.registry",
    "org.peios.services",
    "org.peios.terminal",
}


def request(port: int, method: str, path: str, body=None, cookie: str | None = None):
    headers = {}
    encoded = None
    if body is not None:
        encoded = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    if cookie is not None:
        headers["Cookie"] = f"atrium={cookie}"
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    conn.request(method, path, body=encoded, headers=headers)
    response = conn.getresponse()
    payload = response.read()
    result = response.status, response.getheaders(), payload
    conn.close()
    return result


def json_body(payload: bytes):
    return json.loads(payload.decode())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--server", required=True, type=Path)
    parser.add_argument("--session", required=True, type=Path)
    parser.add_argument("--apps", required=True, type=Path)
    args = parser.parse_args()

    for path in (args.server, args.session, args.apps):
        if not path.exists():
            raise SystemExit(f"missing staged Atrium path: {path}")

    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
    listener.close()

    env = os.environ.copy()
    env["ATRIUM_LISTEN"] = f"127.0.0.1:{port}"
    env["ATRIUM_APPS_DIR"] = str(args.apps)
    fake = Path(__file__).with_name("dev") / "fake_atriumd.py"
    proc = subprocess.Popen(
        [sys.executable, str(fake), str(args.server), str(args.session)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    try:
        deadline = time.monotonic() + 10
        while True:
            if proc.poll() is not None:
                raise RuntimeError(f"fake Atrium stack exited with {proc.returncode}")
            try:
                status, _, _ = request(port, "GET", "/")
                if status == 200:
                    break
            except OSError:
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError("timed out waiting for the staged Atrium server")
            time.sleep(0.05)

        status, _, payload = request(port, "GET", "/api/apps")
        assert status == 401, (status, payload)

        status, _, payload = request(port, "POST", "/api/logon/start", {"username": "package-smoke"})
        assert status == 200, (status, payload)
        prompt = json_body(payload)
        assert prompt["type"] == "prompt" and prompt["prompts"], prompt

        status, headers, payload = request(
            port,
            "POST",
            "/api/logon/answer",
            {
                "conv": prompt["conv"],
                "answers": [{"credential_ref": 7, "data": "secret"}],
            },
        )
        assert status == 200, (status, payload)
        assert json_body(payload)["type"] == "granted", payload
        set_cookie = next(value for name, value in headers if name.lower() == "set-cookie")
        cookie = set_cookie.split(";", 1)[0].split("=", 1)[1]
        assert len(cookie) == 64, set_cookie

        status, _, payload = request(port, "GET", "/api/apps", cookie=cookie)
        assert status == 200, (status, payload)
        apps = json_body(payload)
        assert {app["id"] for app in apps} == EXPECTED_APPS, apps
        for app in apps:
            status, _, payload = request(port, "GET", app["entry"], cookie=cookie)
            assert status == 200 and payload, (app, status)

        for asset in ("/", "/shell/sdk.js", "/shell/tokens.css", "/api/whoami"):
            status, _, payload = request(port, "GET", asset, cookie=cookie)
            assert status == 200 and payload, (asset, status)

        status, _, payload = request(port, "GET", "/apps/org.peios.about/../manifest.toml", cookie=cookie)
        assert status == 404, (status, payload)

        status, headers, _ = request(port, "POST", "/api/logout", {}, cookie=cookie)
        assert status == 303, status
        cleared = next(value for name, value in headers if name.lower() == "set-cookie")
        assert "Max-Age=0" in cleared, cleared
    except Exception:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        output = proc.stdout.read() if proc.stdout is not None else ""
        if output:
            print(output, file=sys.stderr, end="")
        raise
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()

    print(f"Atrium staged integration smoke passed on 127.0.0.1:{port}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
