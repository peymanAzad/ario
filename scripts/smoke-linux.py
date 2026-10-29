#!/usr/bin/env python3
"""Verify that an installed daemon can start aria2 and shut down cleanly."""

import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix="ario-package-smoke-") as tmp:
        root = Path(tmp)
        config = root / "ario"
        config.mkdir()
        port, rpc_port = free_port(), free_port()
        while rpc_port == port:
            rpc_port = free_port()
        (config / "server.toml").write_text(
            f"""[settings]
default_download_location = {json.dumps(tmp)}
category_locations = {{}}
category_extensions = {{}}
proxy = {{ enabled = false }}
start_daemon_on_login = false
stop_daemon_on_close = false
[aria2]
binary_path = "aria2c"
rpc_port = {rpc_port}
download_dir = {json.dumps(tmp)}
""",
            encoding="utf-8",
        )
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with (root / "daemon.log").open("w+") as log:
            process = subprocess.Popen(
                [binary, "--host", "127.0.0.1", "--port", str(port), "--tui-managed"],
                env={**os.environ, "XDG_CONFIG_HOME": tmp},
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            try:
                deadline = time.monotonic() + 30
                while True:
                    try:
                        with opener.open(f"http://127.0.0.1:{port}/health", timeout=2) as response:
                            health = json.load(response)
                        if health["server"] == "ok" and health["aria2_reachable"]:
                            break
                    except (urllib.error.URLError, TimeoutError):
                        pass
                    if process.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError("daemon did not become healthy with aria2 reachable")
                    time.sleep(0.2)
                request = urllib.request.Request(
                    f"http://127.0.0.1:{port}/shutdown", data=b"", method="POST"
                )
                with opener.open(request, timeout=5):
                    pass
                if process.wait(timeout=10) != 0:
                    raise RuntimeError("daemon failed during shutdown")
                print("Daemon startup, aria2 connection, and graceful shutdown passed")
            except Exception:
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
