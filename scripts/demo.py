#!/usr/bin/env python3
"""Prepare and run an isolated, repeatable Ario README recording (Linux, Python 3.11+)."""

from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import ProxyHandler, Request, build_opener, urlopen
import uuid
from zoneinfo import ZoneInfo


REPO = Path(__file__).resolve().parents[1]
MANIFEST = REPO / "demo" / "manifest.json"
DEFAULT_ROOT = REPO / ".demo" / "ario-readme"
CATEGORIES = {"Video", "Music", "Document", "Archive", "Program", "Other"}
LOCAL_HTTP = build_opener(ProxyHandler({}))
FINE_TUNE = {"connections_per_download": 1, "max_connections_per_server": 1,
             "alloc_strategy": "None", "max_retries": 2, "retry_wait_seconds": 2}


class DemoError(RuntimeError):
    pass


@contextmanager
def database_connection(path, **kwargs):
    conn = sqlite3.connect(path, **kwargs)
    try:
        with conn:
            yield conn
    finally:
        conn.close()


def log(message):
    print(message, flush=True)


def read_json(path):
    if path.is_symlink():
        raise DemoError(f"Refusing symlink: {path}")
    return json.loads(path.read_text())


def write_json(path, value):
    if path.is_symlink():
        raise DemoError(f"Refusing symlink: {path}")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=path.name + ".", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(value, stream, indent=2)
            stream.write("\n")
        temporary.replace(path)
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)


def load_manifest(path=MANIFEST):
    manifest = read_json(path)
    items = manifest["downloads"]
    expected = {q["name"]: q["count"] for q in manifest["queues"]}
    if expected != {"Main Queue": 20, "overnight": 10,
                    "creative assets": 10, "developer tools": 10}:
        raise DemoError("Manifest must define the four agreed queues and counts")
    if Counter(item["queue"] for item in items) != expected:
        raise DemoError("Manifest queue counts are incorrect")
    all_items = items + [manifest["clipboard"]]
    if len({item["url"] for item in all_items}) != 51:
        raise DemoError("Expected 50 unique seed URLs and a separate clipboard URL")
    if len({item["id"] for item in all_items}) != 51:
        raise DemoError("Manifest IDs must be unique")
    if {item["category"] for item in items if item["queue"] == "Main Queue"} != CATEGORIES:
        raise DemoError("Main Queue must cover all six categories")
    for item in all_items:
        if (not item["url"].startswith("https://") or
                item["filename"] != item["url"].rsplit("/", 1)[1] or
                Path(item["filename"]).name != item["filename"]):
            raise DemoError(f"Invalid direct file URL or filename: {item['id']}")
        if item["category"] not in CATEGORIES or item["verified_size_bytes"] <= 0:
            raise DemoError(f"Invalid metadata: {item['id']}")
        for key in ("source_page", "license", "license_url", "creator", "attribution"):
            if not item.get(key):
                raise DemoError(f"Missing {key}: {item['id']}")
        if "prepare_bytes" in item and not 0 < item["prepare_bytes"] < item["verified_size_bytes"]:
            raise DemoError(f"Invalid partial transfer target: {item['id']}")
    actual_states = Counter(item["demo_state"] for item in items)
    if actual_states != {"Active": 2, "Paused": 3, "Completed": 8, "Pending": 37}:
        raise DemoError(f"Unexpected recording states: {actual_states}")
    ZoneInfo(manifest["timezone"])
    return manifest


def probe(item, attempts=3, timeout=20):
    """HEAD only; bounded range fallback for servers that reject HEAD."""
    for attempt in range(attempts):
        try:
            request = Request(item["url"], method="HEAD", headers={"User-Agent": "Ario-README-demo/1"})
            try:
                response = urlopen(request, timeout=timeout)
            except HTTPError as error:
                if error.code not in (405, 501):
                    raise
                response = urlopen(Request(item["url"], headers={"Range": "bytes=0-0"}), timeout=timeout)
            with response:
                if response.status not in (200, 206):
                    raise DemoError(f"HTTP {response.status}")
                content_type = response.headers.get_content_type()
                if content_type in ("text/html", "application/xhtml+xml"):
                    raise DemoError("URL returned an HTML page instead of a file")
                content_range = response.headers.get("Content-Range", "")
                size = (int(content_range.rsplit("/", 1)[1]) if content_range
                        else int(response.headers.get("Content-Length", 0)))
                if size <= 0:
                    raise DemoError("Server did not provide a positive file size")
                return {"size_bytes": size, "content_type": content_type,
                        "final_url": response.url,
                        "checked_at": datetime.now(timezone.utc).isoformat()}
        except (OSError, ValueError, URLError, DemoError) as error:
            if attempt == attempts - 1:
                raise DemoError(f"{item['filename']}: {error}") from error
            time.sleep(attempt + 1)


def check_sources(manifest, all_sources=False):
    catalog = manifest["downloads"] + [manifest["clipboard"]]
    items = [item for item in catalog if all_sources or item["demo_state"] != "Pending"]
    verified, failures = {}, []
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = {pool.submit(probe, item): item for item in items}
        for future in as_completed(futures):
            item = futures[future]
            try:
                verified[item["id"]] = future.result() | {"verification": "checked"}
                log(f"[{len(verified) + len(failures)}/{len(items)}] OK {item['filename']}")
            except DemoError as error:
                failures.append(str(error))
                log(f"[{len(verified) + len(failures)}/{len(items)}] FAILED {error}")
    if failures:
        raise DemoError("Unresolved sources; nothing was substituted:\n" + "\n".join(failures))
    for item in catalog:
        if item["id"] not in verified:
            verified[item["id"]] = {"size_bytes": item["verified_size_bytes"],
                                    "verification": "catalog-only", "source_url": item["url"],
                                    "catalog_verified_on": item["verified_on"]}
    if not all_sources:
        log(f"{len(catalog) - len(items)} queued display items retained with catalog metadata; no connection required.")
    return verified


def process_info(pid):
    """Linux process identity, including start time to guard against PID reuse."""
    try:
        proc = Path("/proc") / str(pid)
        fields = (proc / "stat").read_text().rsplit(")", 1)[1].split()
        if fields[0] == "Z":
            return None
        environment = dict(part.split(b"=", 1) for part in
                           (proc / "environ").read_bytes().split(b"\0") if b"=" in part)
        return {"pid": int(pid), "start_ticks": fields[19], "environment": environment,
                "argv": os.fsdecode((proc / "cmdline").read_bytes()).rstrip("\0").split("\0")}
    except (FileNotFoundError, ProcessLookupError):
        return None


def require_plain_tree(path):
    if path.is_symlink():
        raise DemoError(f"Refusing symlink: {path}")
    if path.is_dir():
        for child in path.iterdir():
            require_plain_tree(child)
    elif path.exists() and not path.is_file():
        raise DemoError(f"Refusing special file: {path}")


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class Demo:
    def __init__(self, root, manifest):
        self.root = Path(root).absolute()
        if self.root != self.root.resolve():
            raise DemoError("Demo root and its parents must not be symlinks")
        self.manifest = manifest
        self.config = self.root / "live" / "config" / "ario"
        self.database = self.config / "ario.db"
        self.child = None

    def claim(self):
        marker = self.root / "owner.json"
        if not marker.exists():
            if self.root.exists() and (not self.root.is_dir() or any(self.root.iterdir())):
                raise DemoError(f"Refusing unrelated nonempty directory: {self.root}")
            self.root.mkdir(parents=True, exist_ok=True, mode=0o700)
            write_json(marker, {"kind": "ario-readme-demo", "version": 1,
                                "root": str(self.root), "id": uuid.uuid4().hex})
        self.owner = read_json(marker)
        if (self.owner.get("kind") != "ario-readme-demo" or
                self.owner.get("version") != 1 or self.owner.get("root") != str(self.root)):
            raise DemoError("Demo ownership marker does not match this directory")
        require_plain_tree(self.root)

    @contextmanager
    def lock(self):
        self.claim()
        with (self.root / "command.lock").open("a") as stream:
            try:
                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise DemoError("Another demo command is running") from error
            yield

    def settings(self):
        return read_json(self.root / "settings.json")

    def environment(self, rate="512K"):
        return dict(os.environ, XDG_CONFIG_HOME=str(self.root / "live" / "config"),
                    XDG_DATA_HOME=str(self.root / "live" / "data"),
                    XDG_CACHE_HOME=str(self.root / "live" / "cache"),
                    TZ=self.manifest["timezone"], ARIO_DEMO_ID=self.owner["id"],
                    ARIO_DEMO_LIMIT=rate)

    def owned(self, info):
        return bool(info and info["environment"].get(b"ARIO_DEMO_ID") == self.owner["id"].encode()
                    and info["environment"].get(b"XDG_CONFIG_HOME") ==
                    str(self.root / "live" / "config").encode())

    def workers(self):
        result = []
        for proc in Path("/proc").iterdir():
            if proc.name.isdecimal():
                try:
                    info = process_info(proc.name)
                except PermissionError:
                    continue
                if self.owned(info):
                    result.append(info)
        return result

    def running(self):
        pid_file = self.root / "daemon.json"
        if not pid_file.exists():
            return False
        record = read_json(pid_file)
        info = process_info(record["pid"])
        if info is None:
            return False
        if not self.owned(info) or info["start_ticks"] != record["start_ticks"]:
            raise DemoError("Daemon PID identity changed; refusing to control that process")
        return True

    def api(self, method, route, data=None, timeout=8):
        url = f"http://127.0.0.1:{self.settings()['port']}{route}"
        body = None if data is None else json.dumps(data).encode()
        try:
            request = Request(url, data=body, method=method,
                              headers={"Content-Type": "application/json"})
            with LOCAL_HTTP.open(request, timeout=timeout) as response:
                payload = response.read()
                return json.loads(payload) if payload else None
        except HTTPError as error:
            raise DemoError(f"{method} {route}: HTTP {error.code}: {error.read().decode()}") from error
        except (OSError, URLError) as error:
            raise DemoError(f"{method} {route}: {error}") from error

    def check_endpoint(self):
        settings = self.api("GET", "/settings")
        if settings["default_download_location"] != str(self.root / "live" / "downloads"):
            raise DemoError("Endpoint is not the isolated demo daemon; refusing to control it")

    def init_config(self, args):
        def binary(value, name):
            found = Path(value).resolve() if value else REPO / "target" / "debug" / name
            if not found.is_file() or not os.access(found, os.X_OK):
                raise DemoError(f"Missing executable {found}; run cargo build --workspace --locked")
            return str(found)

        aria2 = shutil.which(args.aria2)
        if not aria2:
            raise DemoError(f"Cannot find aria2c: {args.aria2}")
        port, rpc_port = free_port(), free_port()
        while port == rpc_port:
            rpc_port = free_port()
        settings = {"port": port, "rpc_port": rpc_port,
                    "daemon": binary(args.daemon, "ario_daemon"),
                    "ario": binary(args.ario, "ario"), "aria2": str(Path(aria2).resolve())}
        write_json(self.root / "settings.json", settings)
        self.config.mkdir(parents=True)
        wrapper = self.root / "aria2-demo.sh"
        wrapper.write_text("#!/bin/sh\nexec " + shlex.quote(settings["aria2"]) +
                           ' --no-conf --max-download-limit="${ARIO_DEMO_LIMIT:-512K}"'
                           ' --split=1 --max-connection-per-server=1 --file-allocation=none'
                           ' --continue=true --auto-file-renaming=false --max-concurrent-downloads=5'
                           ' --disable-ipv6=true --connect-timeout=15 --timeout=30 --retry-wait=2 --max-tries=3 "$@"\n')
        wrapper.chmod(0o700)
        # Snapshot of Ario's default mappings; the compatibility test checks these
        # against common/src/enums.rs so the demo does not invent routing rules.
        extensions = {
            "Video": "mp4 mkv avi mov webm flv m4v wmv mpg mpeg m2v 3gp 3g2 ogv ts mts m2ts vob asf".split(),
            "Music": "mp3 flac wav aac ogg m4a opus wma aiff aif alac ape amr oga mka mid midi".split(),
            "Document": "pdf doc docx txt epub odt rtf xls xlsx ods csv ppt pptx odp html htm md tex mobi azw azw3 djvu".split(),
            "Archive": "zip zipx rar 7z tar gz xz bz2 tgz tbz tbz2 txz zst cab iso".split(),
            "Program": "exe msi apk deb rpm app dmg pkg bin run com jar appimage ipa appx msix msixbundle flatpak snap sh bat cmd ps1".split(),
        }
        downloads = self.root / "live" / "downloads"
        lines = ["[aria2]", f"binary_path = {json.dumps(str(wrapper))}",
                 f"rpc_port = {rpc_port}", f"download_dir = {json.dumps(str(downloads))}",
                 "", "[settings]", f"default_download_location = {json.dumps(str(downloads))}",
                 "start_daemon_on_login = false", "stop_daemon_on_close = false", "",
                 "[settings.category_locations]"]
        lines.extend(f"{category} = {json.dumps(str(downloads / category))}" for category in sorted(CATEGORIES))
        lines.extend(["", "[settings.category_extensions]"])
        lines.extend(f"{category} = {json.dumps(values)}" for category, values in extensions.items())
        lines.extend(["", "[settings.proxy]", "enabled = false", ""])
        (self.config / "server.toml").write_text("\n".join(lines))
        (self.config / "tui.toml").write_text(
            f'server_url = "http://127.0.0.1:{port}"\nauto_start_server = false\n'
            'mouse_enabled = true\n'
            '[on-app-exit]\nstop_managed_daemon = "never"\n')
        self.sync_appearance()

    def sync_appearance(self, source=None):
        """Read only visual preferences from the normal config, before XDG isolation."""
        if source is None:
            configured = os.environ.get("XDG_CONFIG_HOME", "")
            config_home = Path(configured) if configured and Path(configured).is_absolute() else Path.home() / ".config"
            source = config_home / "ario" / "tui.toml"
        source = Path(source)
        appearance = tomllib.loads(source.read_text()) if source.is_file() else {}
        path = self.config / "tui.toml"
        if path.resolve() == source.resolve():
            raise DemoError("Normal appearance config must be separate from the demo config")
        current = tomllib.loads(path.read_text())
        lines = [f'server_url = "http://127.0.0.1:{self.settings()["port"]}"',
                 "auto_start_server = false",
                 f'mouse_enabled = {str(current.get("mouse_enabled", True)).lower()}',
                 f'theme = {json.dumps(appearance.get("theme", "default"))}']
        # An absent glyph_mode must stay absent: Ario then uses the same locale
        # based Nerd Font/ASCII selection as the normal application.
        if appearance.get("glyph_mode") is not None:
            lines.append(f'glyph_mode = {json.dumps(appearance["glyph_mode"])}')
        custom = appearance.get("custom_theme", {})
        lines.extend(["", "[custom_theme]"])
        for key in ("foreground", "border", "border_focused", "selected_bg", "selected_fg",
                    "status_ok", "status_error", "status_warning", "text_muted", "accent"):
            if key in custom:
                lines.append(f"{key} = {json.dumps(custom[key])}")
        lines.extend(["", "[on-app-exit]", 'stop_managed_daemon = "never"', ""])
        path.write_text("\n".join(lines))

    def arm_overnight(self):
        """Before boot, suppress the current occurrence to avoid a startup download race."""
        if not self.database.exists():
            return
        now = datetime.now(ZoneInfo(self.manifest["timezone"]))
        occurrence = f"weekly:{now.date()}:00:00:00" if now.hour < 6 else None
        with database_connection(self.database) as conn:
            conn.execute("UPDATE queues SET scheduler_suppressed_occurrence = ?, "
                         "scheduler_active_occurrence = NULL, scheduled_stop_at = NULL, status = 'Paused' "
                         "WHERE name = 'overnight'", (occurrence,))

    def launch(self, rate="512K"):
        if self.running():
            self.check_endpoint()
            return
        if self.workers():
            raise DemoError("Demo processes remain; close the demo UI and run stop first")
        settings = self.settings()
        for port in (settings["port"], settings["rpc_port"]):
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                try:
                    listener.bind(("127.0.0.1", port))
                except OSError as error:
                    raise DemoError(f"Demo port {port} is in use; refusing to reuse its service") from error
        self.arm_overnight()
        with (self.root / "daemon.log").open("ab") as output:
            self.child = subprocess.Popen(
                [settings["daemon"], "--host", "127.0.0.1", "--port", str(settings["port"]), "--tui-managed"],
                env=self.environment(rate), stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                start_new_session=True)
        info = process_info(self.child.pid)
        if info is None:
            raise DemoError(f"Daemon exited; see {self.root / 'daemon.log'}")
        write_json(self.root / "daemon.json", {"pid": self.child.pid, "start_ticks": info["start_ticks"]})
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.child.poll() is not None:
                raise DemoError(f"Daemon exited; see {self.root / 'daemon.log'}")
            try:
                health = self.api("GET", "/health", timeout=1)
                if health["aria2_reachable"]:
                    self.check_endpoint()
                    for queue in self.api("GET", "/queues"):
                        if queue["name"] == "overnight":
                            self.api("POST", f"/queues/{queue['id']}/pause")
                    return
            except DemoError:
                pass
            time.sleep(0.2)
        raise DemoError(f"Daemon was not ready in 15 seconds; see {self.root / 'daemon.log'}")

    def wait_download(self, download_id, predicate, timeout=180):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            live = self.api("GET", f"/downloads/{download_id}")
            status = live["download"]["status"]
            if isinstance(status, dict) or status == "Removed":
                raise DemoError(f"Download {download_id} failed: {status}")
            if predicate(live):
                return live
            time.sleep(0.25)
        raise DemoError(f"Timed out waiting for download {download_id}")

    def pause_all(self):
        for live in self.api("GET", "/downloads"):
            download = live["download"]
            if download["status"] == "Active":
                self.api("POST", f"/downloads/{download['id']}/pause")
                self.wait_download(download["id"], lambda d: d["download"]["status"] != "Active", timeout=15)

    def stop(self):
        failure = None
        try:
            if self.running():
                self.check_endpoint()
                try:
                    self.pause_all()
                except DemoError as error:
                    failure = error
                self.api("POST", "/shutdown")
        finally:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if self.child:
                    self.child.poll()
                workers = [p for p in self.workers() if Path(p["argv"][0]).name != "ario"]
                if not workers:
                    break
                time.sleep(0.2)
            else:
                # Only signal processes whose environment still proves ownership.
                for info in workers:
                    current = process_info(info["pid"])
                    if self.owned(current) and current["start_ticks"] == info["start_ticks"]:
                        os.kill(info["pid"], signal.SIGTERM)
                raise DemoError("Demo workers took too long to stop; retry stop before reset")
        if (self.root / "daemon.json").exists():
            (self.root / "daemon.json").unlink()
        if failure:
            raise failure

    def normalize_stopped(self, verified=None):
        if self.workers():
            raise DemoError("Close demo processes before offline metadata updates")
        with database_connection(self.database) as conn:
            if verified:
                for item in self.manifest["downloads"]:
                    conn.execute("UPDATE downloads SET size = ? WHERE url = ? AND status = 'Pending'",
                                 (verified[item["id"]]["size_bytes"], item["url"]))
            # Transfer IDs belong to the stopped aria2 process. Partial .aria2 files
            # retain real progress and --continue=true resumes them on the next boot.
            conn.execute("UPDATE downloads SET aria2_gid = NULL, manually_started = 0, "
                         "paused_by_scheduler = 0, status = CASE WHEN status = 'Active' "
                         "THEN 'Paused' ELSE status END")
            conn.execute("UPDATE queues SET status = 'Paused', scheduled_stop_at = NULL")
            conn.commit()
            conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        (self.config / "aria2-session.txt").write_text("")

    def seed(self):
        queue_ids = {"Main Queue": 1}
        for position, queue in enumerate(self.manifest["queues"]):
            schedule = queue.get("schedule", {"days": ["Mon"], "start_time": "00:00:00", "end_time": "06:00:00"})
            request = {"name": queue["name"], "position": position,
                       "max_concurrent_downloads": queue["max_concurrent_downloads"],
                       "max_retries": 2, "retry_wait_seconds": 2, "default_finetune": FINE_TUNE,
                       "scheduler_enabled": "schedule" in queue,
                       "recurrence": {"Weekly": schedule}, "run_missed_on_startup": False}
            # Zero capacity prevents even an open schedule window from fetching
            # overnight payloads during preparation. Restore capacity after stopping.
            if queue["name"] == "overnight":
                request["max_concurrent_downloads"] = 0
            if position == 0:
                self.api("PUT", "/queues/1", request)
            else:
                created = self.api("POST", "/queues", request)
                queue_ids[queue["name"]] = created["id"]
            self.api("POST", f"/queues/{queue_ids[queue['name']]}/pause")
        mapping = {}
        for queue in self.manifest["queues"]:
            items = [item for item in self.manifest["downloads"] if item["queue"] == queue["name"]]
            created = self.api("POST", "/downloads", {
                "inputs": [{"Url": item["url"]} for item in items], "queue_id": queue_ids[queue["name"]],
                "finetune_override": None, "start_immediately": False})
            for item, live in zip(items, created, strict=True):
                if live["download"]["category"] != item["category"]:
                    raise DemoError(f"Ario category differs from manifest: {item['filename']}")
                mapping[item["id"]] = live["download"]["id"]
        write_json(self.root / "ids.json", mapping)

    def prepare_group(self, state):
        ids = read_json(self.root / "ids.json")
        items = [item for item in self.manifest["downloads"] if item["demo_state"] == state]
        # A slow preparation pass makes even the short Eerie Choir track pausable.
        self.launch(rate="64K" if state == "Paused" else "512K")
        for item in items:
            log(f"Preparing {state.lower()}: {item['filename']}")
            download_id = ids[item["id"]]
            current = self.api("GET", f"/downloads/{download_id}")
            if state == "Completed" and current["download"]["status"] == "Completed":
                continue
            if state != "Completed" and current["download"]["status"] == "Paused" and current["completed_length"] >= item["prepare_bytes"]:
                continue
            self.api("POST", f"/downloads/{download_id}/resume")
            if state == "Completed":
                self.wait_download(download_id, lambda d: d["download"]["status"] == "Completed")
            else:
                live = self.wait_download(download_id, lambda d: d["completed_length"] >= item["prepare_bytes"])
                if live["download"]["status"] == "Completed":
                    raise DemoError(f"Partial transfer completed too soon: {item['filename']}")
                self.api("POST", f"/downloads/{download_id}/pause")
                self.wait_download(download_id, lambda d: d["download"]["status"] == "Paused", timeout=15)
        self.stop()
        self.normalize_stopped()

    def snapshot(self):
        destination = self.root / "snapshot"
        if destination.exists():
            raise DemoError("Snapshot already exists; use reset or prepare --rebuild")
        require_plain_tree(self.root / "live")
        shutil.copytree(self.root / "live", destination)
        hashes = {}
        for item in self.manifest["downloads"]:
            if item["demo_state"] == "Completed":
                path = self.root / "live" / "downloads" / item["category"] / item["filename"]
                with path.open("rb") as stream:
                    hashes[str(path.relative_to(self.root / "live"))] = hashlib.file_digest(stream, "sha256").hexdigest()
        write_json(self.root / "snapshot.json", {"completed_sha256": hashes,
                                                "manifest_sha256": hashlib.sha256(MANIFEST.read_bytes()).hexdigest()})

    def reset(self):
        if not (self.root / "snapshot" / "config" / "ario" / "ario.db").is_file():
            raise DemoError("No prepared snapshot; run prepare first")
        if any(Path(p["argv"][0]).name == "ario" for p in self.workers()):
            raise DemoError("Close the demo UI before resetting")
        self.stop()
        if self.workers():
            raise DemoError("Demo processes remain; refusing to restore files")
        require_plain_tree(self.root / "live")
        require_plain_tree(self.root / "snapshot")
        shutil.rmtree(self.root / "live")
        shutil.copytree(self.root / "snapshot", self.root / "live")
        if (self.config / "tui.toml").exists():
            self.sync_appearance()
        self.verify()

    def verify(self, clipboard=False):
        if self.running():
            self.check_endpoint()
            lives = self.api("GET", "/downloads")
            rows = [live["download"] | {"completed_length": live["completed_length"]} for live in lives]
            queues = self.api("GET", "/queues")
        else:
            with database_connection(self.database.as_uri() + "?mode=ro", uri=True) as conn:
                conn.row_factory = sqlite3.Row
                rows = [dict(row) for row in conn.execute("SELECT * FROM downloads")]
                queues = [dict(row) for row in conn.execute("SELECT * FROM queues")]
        if len(rows) != (51 if clipboard else 50):
            raise DemoError(f"Expected {51 if clipboard else 50} items, found {len(rows)}")
        by_url = {row["url"]: row for row in rows}
        if len(by_url) != len(rows):
            raise DemoError("Duplicate download URLs")
        queue_names = {queue["id"]: queue["name"] for queue in queues}
        if set(queue_names.values()) != {q["name"] for q in self.manifest["queues"]} or len(queues) != 4:
            raise DemoError("Unexpected queues")
        counts, states = Counter(), {}
        for item in self.manifest["downloads"]:
            if item["url"] not in by_url:
                raise DemoError(f"Missing seed URL: {item['filename']}")
            row = by_url[item["url"]]
            if queue_names[row["queue_id"]] != item["queue"] or row["category"] != item["category"]:
                raise DemoError(f"Wrong queue or category: {item['filename']}")
            expected = item["demo_state"]
            allowed = {expected} | ({"Paused"} if expected == "Active" else set())
            if not isinstance(row["status"], str) or row["status"] not in allowed:
                raise DemoError(f"{item['filename']}: expected {allowed}, got {row['status']}")
            destination = Path(row["destination_path"])
            if not destination.resolve().is_relative_to(self.root / "live" / "downloads"):
                raise DemoError(f"Download path escapes demo: {destination}")
            payload = destination / row["filename"]
            if expected in {"Active", "Paused", "Completed"}:
                if not payload.is_file() or not row["completed_length"]:
                    raise DemoError(f"Missing real downloaded bytes: {payload}")
            if expected == "Completed":
                if payload.stat().st_size != row["size"] or row["completed_length"] != row["size"]:
                    raise DemoError(f"Incomplete completed payload: {payload}")
            if expected == "Pending" and (row["completed_length"] or 0) != 0:
                raise DemoError(f"Pending item has downloaded bytes: {item['filename']}")
            if expected in {"Active", "Paused"} and not 0 < row["completed_length"] < row["size"]:
                raise DemoError(f"Invalid partial progress: {item['filename']}")
            counts[item["queue"]] += 1
            states.setdefault(item["queue"], Counter())[row["status"]] += 1
        overnight = next(queue for queue in queues if queue["name"] == "overnight")
        schedule = (overnight["scheduler"] if "scheduler" in overnight else {
            "enabled": bool(overnight["scheduler_enabled"]),
            "recurrence": json.loads(overnight["scheduler_recurrence"])})
        expected_schedule = next(q["schedule"] for q in self.manifest["queues"] if q["name"] == "overnight")
        if not schedule["enabled"] or schedule["recurrence"] != {"Weekly": expected_schedule}:
            raise DemoError("Overnight schedule differs from the agreed daily 00:00–06:00 window")
        for queue in self.manifest["queues"]:
            if counts[queue["name"]] != queue["count"]:
                raise DemoError(f"Wrong item count: {queue['name']}")
        snapshot = self.root / "snapshot.json"
        if snapshot.exists():
            for relative, expected_hash in read_json(snapshot)["completed_sha256"].items():
                with (self.root / "live" / relative).open("rb") as stream:
                    if hashlib.file_digest(stream, "sha256").hexdigest() != expected_hash:
                        raise DemoError(f"Completed file changed: {relative}")
        for name in counts:
            log(f"{name}: {counts[name]} items — " + ", ".join(f"{n} {s.lower()}" for s, n in states[name].items()))
        if clipboard:
            item = by_url.get(self.manifest["clipboard"]["url"])
            if not item or item["category"] != "Video" or item["status"] != "Completed" or queue_names[item["queue_id"]] != "Main Queue":
                raise DemoError("Clipboard item did not complete in Main Queue as Video")
        log("Verified real files, progress, categories, queue counts, and overnight schedule.")

    def import_clipboard(self):
        if not self.running():
            raise DemoError("Start the demo before testing clipboard import")
        self.verify()
        item = self.manifest["clipboard"]
        created = self.api("POST", "/downloads", {"inputs": [{"Url": item["url"]}], "queue_id": 1,
                                                 "finetune_override": None, "start_immediately": True})
        self.wait_download(created[0]["download"]["id"], lambda d: d["download"]["status"] == "Completed", timeout=90)
        self.verify(clipboard=True)
        log("Clipboard import path produced item 51. Run reset to remove it and restore the recording state.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="isolated demo directory")
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare", help="check sources and prepare real demo downloads")
    recovery = prepare.add_mutually_exclusive_group()
    recovery.add_argument("--rebuild", action="store_true", help="replace only this owned demo's preparation")
    recovery.add_argument("--resume", action="store_true", help="finish an interrupted preparation, retaining real downloads")
    prepare.add_argument("--daemon", help="ario_daemon binary (default: target/debug/ario_daemon)")
    prepare.add_argument("--ario", help="ario binary (default: target/debug/ario)")
    prepare.add_argument("--aria2", default="aria2c", help="aria2c executable")
    start = commands.add_parser("start", help="resume the two recording transfers and open Ario")
    start.add_argument("--no-ui", action="store_true", help="start daemon/transfers without opening the TUI")
    commands.add_parser("ui", help="open the demo TUI without resuming transfers")
    commands.add_parser("stop", help="pause real transfers and stop only the demo daemon")
    commands.add_parser("reset", help="restore the prepared database and downloaded payloads")
    verify = commands.add_parser("verify", help="verify dataset and real downloaded files")
    verify.add_argument("--clipboard", action="store_true", help="exercise URL import and verify item 51")
    commands.add_parser("check-sources", help="recheck all 51 links without preparing a database")
    commands.add_parser("clipboard", help="print the reserved clipboard URL")
    args = parser.parse_args()
    if sys.platform != "linux":
        raise DemoError("This isolated demo runner currently supports Linux")
    manifest = load_manifest()
    if args.command == "clipboard":
        log(manifest["clipboard"]["url"])
        return
    if args.command == "check-sources":
        check_sources(manifest, all_sources=True)
        return
    if args.command in {"start", "ui"} and not getattr(args, "no_ui", False) and not sys.stdin.isatty():
        raise DemoError("The TUI needs an interactive terminal; use start --no-ui for headless operation")
    demo = Demo(args.root, manifest)
    with demo.lock():
        if args.command == "prepare":
            existing = (demo.root / "settings.json").exists()
            if existing and not (args.rebuild or args.resume):
                raise DemoError("Demo already exists; use reset, prepare --resume, or prepare --rebuild")
            if args.resume and (not existing or not (demo.root / "ids.json").exists()):
                raise DemoError("No seeded preparation to resume; use prepare --rebuild")
            if args.resume and (demo.root / "snapshot.json").exists():
                raise DemoError("Preparation is already complete; use reset")
            verified = check_sources(manifest)
            if existing:
                if any(Path(p["argv"][0]).name == "ario" for p in demo.workers()):
                    raise DemoError("Close the demo UI before rebuilding")
                demo.stop()
                if args.rebuild:
                    for name in ("live", "snapshot"):
                        if (demo.root / name).exists():
                            shutil.rmtree(demo.root / name)
                    for name in ("settings.json", "ids.json", "snapshot.json"):
                        (demo.root / name).unlink(missing_ok=True)
                else:
                    ids = read_json(demo.root / "ids.json")
                    with database_connection(demo.database.as_uri() + "?mode=ro", uri=True) as conn:
                        seeded = dict(conn.execute("SELECT id, url FROM downloads"))
                    expected = {ids[item["id"]]: item["url"] for item in manifest["downloads"]}
                    if seeded != expected:
                        raise DemoError("Existing seed differs from the manifest; use prepare --rebuild")
            write_json(demo.root / "verified-sources.json", verified)
            try:
                if not args.resume:
                    demo.init_config(args)
                    demo.launch()
                    demo.seed()
                    demo.stop()
                demo.normalize_stopped(verified)
                for state in ("Completed", "Paused", "Active"):
                    demo.prepare_group(state)
                # All transfers are stopped; restore overnight capacity without
                # an enable/reconcile race while its actual schedule stays enabled.
                with database_connection(demo.database) as conn:
                    conn.execute("UPDATE queues SET max_concurrent_downloads = 2 WHERE name = 'overnight'")
                demo.verify()
                demo.snapshot()
                (demo.root / "preparation-incomplete.json").unlink(missing_ok=True)
            except BaseException:
                try:
                    demo.stop()
                except DemoError as cleanup_error:
                    log(f"Cleanup needs attention: {cleanup_error}")
                raise
            log(f"Prepared database: {demo.database}")
            log("Start recording: python3 scripts/demo.py" +
                ("" if demo.root == DEFAULT_ROOT else " --root " + shlex.quote(str(demo.root))) + " start")
            log("Clipboard URL: " + manifest["clipboard"]["url"])
        elif args.command == "start":
            if not (demo.root / "snapshot.json").exists():
                raise DemoError("Run prepare first")
            demo.sync_appearance()
            demo.launch()
            ids = read_json(demo.root / "ids.json")
            for item in manifest["downloads"]:
                if item["demo_state"] == "Active":
                    download_id = ids[item["id"]]
                    current = demo.api("GET", f"/downloads/{download_id}")
                    if current["download"]["status"] == "Paused":
                        demo.api("POST", f"/downloads/{download_id}/resume")
                    elif current["download"]["status"] != "Active":
                        raise DemoError("Recording transfer is no longer paused/active; use reset")
                    demo.wait_download(download_id, lambda d: d["download_speed"] > 0, timeout=60)
            demo.verify()
            log("Clipboard URL: " + manifest["clipboard"]["url"])
            log(f"Demo endpoint: http://127.0.0.1:{demo.settings()['port']}")
        elif args.command == "stop":
            demo.stop()
            if demo.database.exists() and not demo.workers():
                demo.normalize_stopped()
            log("Demo stopped.")
        elif args.command == "reset":
            demo.reset()
            log("Restored the 50-item snapshot; run start for the next take.")
        elif args.command == "verify":
            if args.clipboard:
                demo.import_clipboard()
            else:
                demo.verify()
        elif args.command == "ui":
            if not demo.running():
                raise DemoError("Start the demo first")
            demo.sync_appearance()
            demo.check_endpoint()
    # Release the command lock so another terminal can stop the demo while the
    # foreground UI is open. Configuration remains scoped to this child only.
    if args.command == "ui" or (args.command == "start" and not args.no_ui):
        subprocess.run([demo.settings()["ario"]], env=demo.environment(), check=True)


if __name__ == "__main__":
    try:
        main()
    except (DemoError, OSError, sqlite3.Error, ValueError) as error:
        print(f"demo: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print("Demo command interrupted. Run stop before resetting or rebuilding.", file=sys.stderr)
        sys.exit(130)
