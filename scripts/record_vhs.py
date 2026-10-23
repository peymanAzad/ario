#!/usr/bin/env python3
"""Render the prepared Ario demo through VHS, then restore its starting snapshot."""

import argparse
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys

from demo import DEFAULT_ROOT, REPO, Demo, DemoError, load_manifest


TOOLS = REPO / ".demo" / "vhs-tools"


def executable(name, cached=None):
    found = shutil.which(name)
    if found:
        return found
    candidate = TOOLS / (cached or name)
    if candidate.is_file() and os.access(candidate, os.X_OK):
        return str(candidate)
    raise DemoError(f"Install {name} before recording; see docs/demo.md")


def recording_environment(display, binaries):
    env = dict(os.environ, DISPLAY=f":{display}", XDG_SESSION_TYPE="x11",
               TERM="xterm-256color", COLORTERM="truecolor")
    # Codex's command environment disables ANSI color. The terminal recorded
    # by VHS supports true color and must render the user's actual TUI theme.
    env.pop("NO_COLOR", None)
    env.pop("WAYLAND_DISPLAY", None)
    directories = list(dict.fromkeys(str(Path(path).parent) for path in binaries.values()))
    env["PATH"] = os.pathsep.join(directories + [env.get("PATH", "")])
    return env


def finalize_recording(ffmpeg, output):
    # VHS's window-bar image stream can make FFmpeg emit 25 FPS even when
    # capture is configured for 15. Normalize the exported GIF explicitly.
    mp4 = output / ".capture.mp4"
    palette = output / ".palette.png"
    # Separate palette generation avoids buffering the entire long recording
    # in memory while the palette filter waits for the final frame.
    subprocess.run([
        ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", str(mp4),
        "-vf", "fps=15,palettegen=stats_mode=diff", "-frames:v", "1",
        "-update", "1", str(palette),
    ], check=True, timeout=180)
    subprocess.run([
        ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", str(mp4),
        "-i", str(palette), "-filter_complex",
        "[0:v]fps=15[v];[v][1:v]paletteuse=dither=none",
        "-loop", "0", str(output / "ario-readme.gif"),
    ], check=True, timeout=180)
    with (output / "ario-readme.gif").open("rb") as gif:
        if gif.read(6) not in (b"GIF87a", b"GIF89a"):
            raise DemoError("Recording output is not a GIF")
    # VHS screenshots taken after Hide/Show can reference an earlier frame.
    # Derive the opening and final stills directly from the exported video.
    for seek, filename in ((["-ss", "1"], "main.png"), (["-sseof", "-2"], "download.png")):
        subprocess.run([
            ffmpeg, "-hide_banner", "-loglevel", "error", "-y", *seek,
            "-i", str(mp4), "-frames:v", "1", str(output / filename),
        ], check=True, timeout=30)
    mp4.unlink()
    palette.unlink()
    # Remove the MP4 delivered by the earlier version of this recorder so the
    # recordings folder cannot suggest it is still the intended README asset.
    (output / "ario.mp4").unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font", default="JetBrainsMono Nerd Font Mono")
    args = parser.parse_args()
    binaries = {
        "vhs": executable("vhs"), "ttyd": executable("ttyd"),
        "ffmpeg": executable("ffmpeg"), "xsel": executable("xsel"),
        "xvfb": executable("Xvfb", "xvfb-root/usr/bin/Xvfb"),
    }
    if not shutil.which("google-chrome") and not shutil.which("chromium") and not shutil.which("chromium-browser"):
        raise DemoError("Install Chrome or Chromium before recording")
    if subprocess.check_output(["fc-match", "-f", "%{family}", args.font], text=True).strip().split(",")[0] != args.font:
        raise DemoError(f"Font {args.font!r} not found; choose an installed font with --font")

    demo = Demo(DEFAULT_ROOT, load_manifest())
    with demo.lock():
        if not (demo.root / "snapshot.json").exists():
            raise DemoError("Prepare the demo with python3 scripts/demo.py prepare first")
        demo.reset()
        appearance = (demo.config / "tui.toml").read_text()
    import tomllib
    theme_name = tomllib.loads(appearance).get("theme", "default")
    terminal_theme = {
        "catppuccin-frappe": "Catppuccin Frappe", "catppuccin-frappé": "Catppuccin Frappe",
        "catppuccin-latte": "Catppuccin Latte", "catppuccin-macchiato": "Catppuccin Macchiato",
        "catppuccin-mocha": "Catppuccin Mocha",
    }.get(theme_name.lower(), "Catppuccin Frappe")
    output = demo.root / "recordings"
    output.mkdir(exist_ok=True)
    template = (REPO / "demo" / "ario.tape").read_text().splitlines()
    # VHS derives encoder options from top-level settings before executing any
    # Source commands, so write one flat tape with the user's appearance.
    tape = output / "ario.tape"
    tape.write_text("\n".join(
        f"Set FontFamily {json.dumps(args.font)}" if line.startswith("Set FontFamily ") else
        f"Set Theme {json.dumps(terminal_theme)}" if line.startswith("Set Theme ") else line
        for line in template) + "\n")

    # Xvfb owns its own X selection. Neither Copy in the tape nor arboard in
    # Ario touches the user's actual Wayland/X11 clipboard.
    read_fd, write_fd = os.pipe()
    xvfb = None
    try:
        with (output / "xvfb.log").open("wb") as xvfb_log:
            xvfb = subprocess.Popen(
                [binaries["xvfb"], "-displayfd", str(write_fd), "-screen", "0", "1440x900x24",
                 "-nolisten", "tcp", "-ac"], pass_fds=(write_fd,),
                stdout=xvfb_log, stderr=xvfb_log)
        os.close(write_fd)
        write_fd = None
        if not select.select([read_fd], [], [], 10)[0]:
            raise DemoError(f"Private display did not start; see {output / 'xvfb.log'}")
        display = os.read(read_fd, 32).decode().strip()
        if not display.isdecimal():
            raise DemoError(f"Private display failed; see {output / 'xvfb.log'}")
        env = recording_environment(display, binaries)
        print(f"Recording on private display :{display}, theme {theme_name}, 15 FPS.", flush=True)
        with demo.lock():
            demo.launch()
            ids = json.loads((demo.root / "ids.json").read_text())
            for item in demo.manifest["downloads"]:
                if item["demo_state"] == "Active":
                    download_id = ids[item["id"]]
                    demo.api("POST", f"/downloads/{download_id}/resume")
                    demo.wait_download(download_id, lambda row: row["download_speed"] > 0, timeout=60)
        subprocess.run([binaries["vhs"], str(tape)], cwd=REPO, env=env, check=True, timeout=480)
        for filename in (".capture.mp4",):
            if not (output / filename).is_file() or not (output / filename).stat().st_size:
                raise DemoError(f"VHS did not create {filename}; inspect demo/ario.tape outputs")
        finalize_recording(binaries["ffmpeg"], output)
        # The recording must perform a real import, not merely show its dialog.
        with demo.lock():
            rows = demo.api("GET", "/downloads")
            imported = [row for row in rows if row["download"]["url"] == demo.manifest["clipboard"]["url"]]
            if len(rows) != 51 or len(imported) != 1:
                raise DemoError("The tape did not import its reserved URL; inspect the clipboard screenshot")
            if isinstance(imported[0]["download"]["status"], dict):
                raise DemoError(f"Clipboard transfer failed: {imported[0]['download']['status']}")
            if imported[0]["completed_length"] <= 0:
                raise DemoError("Clipboard item was imported but did not download any bytes")
            queues = demo.api("GET", "/queues")
            if not any(queue["name"] == "weekend projects" for queue in queues):
                raise DemoError("The tape did not save the demonstrated queue")
        print(f"GIF: {output / 'ario-readme.gif'}", flush=True)
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)
        if xvfb is not None:
            xvfb.terminate()
            try:
                xvfb.wait(timeout=5)
            except subprocess.TimeoutExpired:
                xvfb.kill()
                xvfb.wait()
        with demo.lock():
            demo.reset()


if __name__ == "__main__":
    try:
        main()
    except (DemoError, OSError, subprocess.SubprocessError) as error:
        print(f"record_vhs: {error}", file=sys.stderr)
        sys.exit(1)
