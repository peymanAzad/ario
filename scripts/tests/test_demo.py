"""Offline safety and compatibility checks for the README demo runner."""

from contextlib import redirect_stdout
import copy
from datetime import datetime
import importlib.util
import io
import json
from pathlib import Path
import re
import tempfile
import tomllib
import unittest
from unittest.mock import patch


REPO = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ario_demo", REPO / "scripts" / "demo.py")
demo = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(demo)


class DemoTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.manifest = demo.load_manifest()

    def make_demo(self):
        instance = demo.Demo(self.base / "recording", self.manifest)
        instance.claim()
        return instance

    def fixture(self, instance):
        instance.config.mkdir(parents=True)
        with demo.database_connection(instance.database) as conn:
            conn.executescript((REPO / "server" / "schema.sql").read_text())
            for index, queue in enumerate(self.manifest["queues"], 1):
                if index > 1:
                    conn.execute("INSERT INTO queues (id, name, position, created_at) VALUES (?, ?, ?, ?)",
                                 (index, queue["name"], index - 1, "2026-10-22T00:00:00Z"))
                if queue["name"] == "overnight":
                    conn.execute("UPDATE queues SET scheduler_enabled = 1, scheduler_recurrence = ? WHERE id = ?",
                                 (json.dumps({"Weekly": queue["schedule"]}), index))
            for index, item in enumerate(self.manifest["downloads"], 1):
                state = "Paused" if item["demo_state"] == "Active" else item["demo_state"]
                destination = instance.root / "live" / "downloads" / item["category"]
                destination.mkdir(parents=True, exist_ok=True)
                size, completed = (4, 0) if state == "Pending" else (4, 1)
                if state == "Completed":
                    completed = size
                if state != "Pending":
                    (destination / item["filename"]).write_bytes(b"x" * completed)
                queue_id = next(n for n, q in enumerate(self.manifest["queues"], 1) if q["name"] == item["queue"])
                conn.execute("INSERT INTO downloads (id, url, filename, destination_path, source_type, "
                             "category, status, size, completed_length, queue_id, created_at) "
                             "VALUES (?, ?, ?, ?, 'Http', ?, ?, ?, ?, ?, ?)",
                             (index, item["url"], item["filename"], str(destination), item["category"],
                              state, size, completed, queue_id, "2026-10-22T00:00:00Z"))

    def test_manifest_has_required_counts_states_and_reserved_url(self):
        self.assertEqual(len(self.manifest["downloads"]), 50)
        changed = copy.deepcopy(self.manifest)
        changed["clipboard"]["url"] = changed["downloads"][0]["url"]
        path = self.base / "invalid.json"
        path.write_text(json.dumps(changed))
        with self.assertRaisesRegex(demo.DemoError, "unique seed URLs"):
            demo.load_manifest(path)

    def test_preparation_never_contacts_queued_sources(self):
        def successful_probe(item):
            self.assertNotEqual(item["demo_state"], "Pending")
            return {"size_bytes": item["verified_size_bytes"]}
        with patch.object(demo, "probe", side_effect=successful_probe) as probe, redirect_stdout(io.StringIO()):
            result = demo.check_sources(self.manifest)
        self.assertEqual(probe.call_count, 14)
        self.assertEqual(len(result), 51)
        for item in self.manifest["downloads"]:
            if item["demo_state"] == "Pending":
                self.assertEqual(result[item["id"]]["verification"], "catalog-only")
                self.assertNotIn("checked_at", result[item["id"]])
                self.assertEqual(result[item["id"]]["source_url"], item["url"])

    def test_optional_source_audit_checks_all_51_urls(self):
        with patch.object(demo, "probe", return_value={"size_bytes": 10}) as probe, redirect_stdout(io.StringIO()):
            result = demo.check_sources(self.manifest, all_sources=True)
        self.assertEqual(probe.call_count, 51)
        self.assertTrue(all(item["verification"] == "checked" for item in result.values()))

    def test_unrelated_directory_and_symlink_are_refused(self):
        path = self.base / "personal"
        path.mkdir()
        important = path / "keep.txt"
        important.write_text("untouched")
        with self.assertRaises(demo.DemoError):
            demo.Demo(path, self.manifest).claim()
        link = self.base / "link"
        link.symlink_to(path, target_is_directory=True)
        with self.assertRaises(demo.DemoError):
            demo.Demo(link, self.manifest)
        self.assertEqual(important.read_text(), "untouched")

    def test_config_uses_current_ario_defaults_and_isolated_paths(self):
        instance = self.make_demo()
        args = type("Args", (), {"daemon": "/bin/true", "ario": "/bin/true", "aria2": "/bin/true"})()
        with patch.object(demo, "free_port", side_effect=[49170, 49171]):
            instance.init_config(args)
        config = tomllib.loads((instance.config / "server.toml").read_text())
        text = (REPO / "common" / "src" / "enums.rs").read_text()
        defaults = dict((category, re.findall(r'"([a-z0-9]+)"', values)) for category, values in
                        re.findall(r'FileCategory::(\w+),\s*vec!\[(.*?)\]', text, re.S))
        self.assertEqual(config["settings"]["category_extensions"], defaults)
        for path in config["settings"]["category_locations"].values():
            self.assertTrue(Path(path).is_relative_to(instance.root / "live" / "downloads"))
        env_before = dict(demo.os.environ)
        child_env = instance.environment()
        self.assertEqual(child_env["TZ"], "Asia/Tbilisi")
        self.assertEqual(child_env["XDG_CONFIG_HOME"], str(instance.root / "live" / "config"))
        self.assertEqual(dict(demo.os.environ), env_before)
        tui = tomllib.loads((instance.config / "tui.toml").read_text())
        self.assertFalse(tui["auto_start_server"])

    def test_pid_reuse_is_not_treated_as_our_daemon(self):
        instance = self.make_demo()
        demo.write_json(instance.root / "daemon.json", {"pid": 123, "start_ticks": "before"})
        info = {"pid": 123, "start_ticks": "after", "environment": {}, "argv": ["unrelated"]}
        with patch.object(demo, "process_info", return_value=info):
            with self.assertRaisesRegex(demo.DemoError, "PID identity changed"):
                instance.running()

    def test_visual_settings_sync_without_copying_normal_endpoint_or_modifying_source(self):
        instance = self.make_demo()
        args = type("Args", (), {"daemon": "/bin/true", "ario": "/bin/true", "aria2": "/bin/true"})()
        with patch.object(demo, "free_port", side_effect=[49170, 49171]):
            instance.init_config(args)
        source = self.base / "normal-tui.toml"
        original = ('server_url = "http://127.0.0.1:47812"\nauto_start_server = true\n'
                    'theme = "catppuccin-frappe"\nglyph_mode = "nerd"\n'
                    '[custom_theme]\naccent = "#abcdef"\n'
                    '[on-app-exit]\nstop_managed_daemon = "always"\n')
        source.write_text(original)
        instance.sync_appearance(source)
        actual = tomllib.loads((instance.config / "tui.toml").read_text())
        self.assertEqual(actual["theme"], "catppuccin-frappe")
        self.assertEqual(actual["glyph_mode"], "nerd")
        self.assertEqual(actual["custom_theme"], {"accent": "#abcdef"})
        self.assertEqual(actual["server_url"], "http://127.0.0.1:49170")
        self.assertFalse(actual["auto_start_server"])
        self.assertEqual(actual["on-app-exit"]["stop_managed_daemon"], "never")
        self.assertEqual(source.read_text(), original)
        # Removing the setting restores native automatic icon selection, rather
        # than retaining the demo's previous explicit glyph mode or color override.
        source.write_text('theme = "catppuccin-frappe"\n')
        instance.sync_appearance(source)
        actual = tomllib.loads((instance.config / "tui.toml").read_text())
        self.assertNotIn("glyph_mode", actual)
        self.assertEqual(actual["custom_theme"], {})

    def test_overnight_is_suppressed_before_first_poll_inside_window(self):
        instance = self.make_demo()
        self.fixture(instance)
        with patch.object(demo, "datetime") as clock:
            clock.now.return_value = datetime.fromisoformat("2026-10-22T02:30:00+04:00")
            instance.arm_overnight()
        with demo.database_connection(instance.database) as conn:
            suppressed = conn.execute("SELECT scheduler_suppressed_occurrence FROM queues WHERE name = 'overnight'").fetchone()[0]
        self.assertEqual(suppressed, "weekly:2026-10-22:00:00:00")

    def test_reset_restores_payloads_and_database_and_preserves_external_data(self):
        instance = self.make_demo()
        self.fixture(instance)
        external = self.base / "personal-download.txt"
        external.write_text("keep me")
        instance.snapshot()
        first = next(item for item in self.manifest["downloads"] if item["demo_state"] == "Completed")
        payload = instance.root / "live" / "downloads" / first["category"] / first["filename"]
        payload.write_bytes(b"changed")
        extra = instance.root / "live" / "downloads" / "extra.txt"
        extra.write_text("clipboard take")
        with demo.database_connection(instance.database) as conn:
            conn.execute("DELETE FROM downloads WHERE id = 1")
        with patch.object(instance, "workers", return_value=[]), redirect_stdout(io.StringIO()):
            instance.reset()
        self.assertEqual(payload.read_bytes(), b"xxxx")
        self.assertFalse(extra.exists())
        self.assertEqual(external.read_text(), "keep me")
        with demo.database_connection(instance.database) as conn:
            self.assertEqual(conn.execute("SELECT count(*) FROM downloads").fetchone()[0], 50)

    def test_symlinked_payload_aborts_reset_without_touching_target(self):
        instance = self.make_demo()
        self.fixture(instance)
        instance.snapshot()
        external = self.base / "outside.txt"
        external.write_text("do not change")
        (instance.root / "live" / "downloads" / "unsafe-link").symlink_to(external)
        with patch.object(instance, "workers", return_value=[]):
            with self.assertRaisesRegex(demo.DemoError, "symlink"):
                instance.reset()
        self.assertEqual(external.read_text(), "do not change")

    def test_stopped_metadata_update_preserves_progress_and_clears_session_ids(self):
        instance = self.make_demo()
        self.fixture(instance)
        with demo.database_connection(instance.database) as conn:
            conn.execute("UPDATE downloads SET aria2_gid = 'old', status = 'Active', completed_length = 1 WHERE id = 1")
        with patch.object(instance, "workers", return_value=[]):
            instance.normalize_stopped()
        with demo.database_connection(instance.database) as conn:
            row = conn.execute("SELECT aria2_gid, status, completed_length FROM downloads WHERE id = 1").fetchone()
        self.assertEqual(row, (None, "Paused", 1))
        self.assertEqual((instance.config / "aria2-session.txt").read_text(), "")

    def test_verification_rejects_completed_metadata_without_real_payload(self):
        instance = self.make_demo()
        self.fixture(instance)
        item = next(item for item in self.manifest["downloads"] if item["demo_state"] == "Completed")
        payload = instance.root / "live" / "downloads" / item["category"] / item["filename"]
        payload.unlink()
        with self.assertRaisesRegex(demo.DemoError, "Missing real downloaded bytes"):
            instance.verify()

    def test_verification_rejects_download_path_outside_demo(self):
        instance = self.make_demo()
        self.fixture(instance)
        with demo.database_connection(instance.database) as conn:
            conn.execute("UPDATE downloads SET destination_path = ? WHERE id = 1", (str(self.base),))
        with self.assertRaisesRegex(demo.DemoError, "escapes demo"):
            instance.verify()


if __name__ == "__main__":
    unittest.main()
