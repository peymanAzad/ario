"""Keep recording terminal capabilities separate from the host environment."""

import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from record_vhs import recording_environment


class RecordingEnvironmentTests(unittest.TestCase):
    def test_recording_restores_color_without_changing_host_or_config_source(self):
        host = {"NO_COLOR": "1", "TERM": "dumb", "COLORTERM": "",
                "WAYLAND_DISPLAY": "wayland-1", "DISPLAY": ":0",
                "XDG_CONFIG_HOME": "/normal/config", "PATH": "/usr/bin"}
        with patch.dict(os.environ, host, clear=True):
            recorded = recording_environment("42", {"vhs": "/local/tools/vhs"})
            self.assertEqual(dict(os.environ), host)
        self.assertNotIn("NO_COLOR", recorded)
        self.assertNotIn("WAYLAND_DISPLAY", recorded)
        self.assertEqual(recorded["TERM"], "xterm-256color")
        self.assertEqual(recorded["COLORTERM"], "truecolor")
        self.assertEqual(recorded["DISPLAY"], ":42")
        self.assertEqual(recorded["XDG_CONFIG_HOME"], host["XDG_CONFIG_HOME"])
        self.assertEqual(recorded["PATH"], "/local/tools:/usr/bin")
