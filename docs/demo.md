# Recording the Ario README demo

The demo uses 50 public downloads in four queues: **Main Queue** (20),
**overnight** (10), **creative assets** (10), and **developer tools** (10).
Main Queue includes Video, Music, Document, Archive, Program, and Other.
The separate Sintel trailer URL is reserved for importing from the clipboard.

## Prepare once

Requirements: Linux, Python 3.11 or newer, Rust/Cargo, `aria2c`, network access,
and approximately 150 MB of free disk space for the prepared files and snapshot.
The entire catalog is about 6 GB, but preparation downloads only selected small
files and partial transfers. No packages or applications in the catalog are installed.

From the repository root:

```sh
cargo build --workspace --locked
python3 scripts/demo.py prepare
```

Preparation checks the 14 sources used for completed files, partial transfers,
and clipboard import, creates queues through Ario's API, and performs real
downloads. The other 37 items remain queued with their original URLs and previously
verified catalog sizes; their servers do not need to be reachable for recording.
This includes all eight GNU manuals. It usually takes a few minutes; source
timeouts can make it slower. A required source that still fails stops preparation
with its filename and error. URLs are never silently replaced. If seeding already finished,
resume while retaining completed files and partial progress:

```sh
python3 scripts/demo.py prepare --resume
```

To recreate a failed or unwanted preparation from scratch:

```sh
python3 scripts/demo.py prepare --rebuild
```

Runtime files live in `.demo/ario-readme/`, which Git ignores. The database is
`.demo/ario-readme/live/config/ario/ario.db`. The runner scopes XDG configuration,
data, cache, timezone, and process identity to its children, allocates separate
loopback ports, and directs every category to the demo download directory.
The existing Ario configuration, database, downloads, and environment are untouched.

Use `--root /absolute/path` **before the command** for a different location:

```sh
python3 scripts/demo.py --root /tmp/ario-recording prepare
python3 scripts/demo.py --root /tmp/ario-recording start
```

The runner refuses unrelated nonempty directories, symlinks, mismatched process
identities, and occupied ports. `prepare --rebuild` replaces only an owned demo.
Optional `prepare --daemon PATH --ario PATH --aria2 PATH` selects different binaries.

## Record automatically with VHS

```sh
python3 scripts/record_vhs.py
```

This runs [`demo/ario.tape`](../demo/ario.tape) through
[VHS](https://github.com/charmbracelet/vhs) at **15 FPS**, with your current Ario
theme and icons and the installed JetBrainsMono Nerd Font Mono. The recorder
enables true color only in its child process, clearing inherited `NO_COLOR`.
It shows selection highlights in all three panes, pauses/resumes a movie,
creates a queue with weekend scheduling, opens the overnight scheduler,
and imports the reserved trailer from a private clipboard. Queue creation
and clipboard import show both tabs, with 4–5 second pauses to read the options.
The demo snapshot is restored after recording so the next take starts with
the same 50 items.
Close any open demo UI before running it.

The final output is `.demo/ario-readme/recordings/ario-readme.gif`, an animated GIF
for the GitHub README. An intermediate `.capture.mp4` is removed after encoding.
The same folder contains screenshots of the main view, scheduler, clipboard
dialog, and imported download. Edit the tape to adjust actions and pauses.
Use `--font "Another Installed Font"` to select a different font.

The wrapper needs VHS, ttyd, FFmpeg, Chrome/Chromium, Xvfb, xsel, and the selected
font. This workspace has local VHS and ttyd binaries in the ignored
`.demo/vhs-tools/` folder; VHS's release checksum was verified. It also has an
extracted, signature-checked Fedora Xvfb package there. System-installed tools
take precedence when available. On another machine, install those tools first.

The recording uses a separate headless browser profile and a private X display.
Clipboard operations happen on that display, leaving your desktop clipboard
alone. The wrapper generates a flat tape with your appearance settings in the
recordings folder, so run the wrapper instead of invoking the tape directly.
The wrapper encodes the final GIF at 15 FPS with FFmpeg and checks its file signature.

## Record a take manually

```sh
python3 scripts/demo.py start
```

This resumes the two Main Queue movie downloads, waits for live speed, verifies
the dataset, and opens Ario with your normal theme, custom colors, and icon settings.
Use a fixed terminal size around 120 columns by 32 rows, with readable text.
Preparation, `start`, `ui`, and `reset` copy only `theme`, `custom_theme`, and
`glyph_mode` from your normal Ario `tui.toml` (using your existing `XDG_CONFIG_HOME`,
or `~/.config/ario/tui.toml`). If `glyph_mode` is unset, the demo uses Ario's normal
locale-based icon selection. The demo keeps its own endpoint and daemon controls;
your normal configuration file is read without being changed.

| Queue | Recording state |
| --- | --- |
| Main Queue | 2 downloading, 3 paused, 3 completed, 12 pending |
| overnight | 10 pending, daily schedule enabled |
| creative assets | 3 completed, 7 pending |
| developer tools | 2 completed, 8 pending |

The completed Main items are jq, Icy Adventure, and the studio HDR.
Bash and Make source archives and Eerie Choir have real partial
downloads. Elephants Dream and Tears of Steel resume for the opening shot.
The creative HDRs and curl and wget packages are also genuinely complete.
The GNU manuals stay pending; none is needed for the live portion of the GIF.

Recording transfers use one connection and a **512 KiB/s per-download cap**.
The preparation pass for the three paused examples runs at 64 KiB/s to make
the short music file reliably pausable. Progress and speed come from aria2.

The overnight schedule runs every day from **00:00 to 06:00 Asia/Tbilisi**.
During preparation its capacity is held at zero. Once preparation finishes,
capacity is restored to two while the daemon is stopped. Before each launch,
the runner arms suppression for any currently open occurrence; after startup
it also calls Ario's queue Pause action. This prevents a startup race from
starting the large downloads. A future midnight starts the next occurrence
naturally, so stop the demo after recording.

Use three or four live items: Elephants Dream, Tears of Steel, the clipboard
trailer, and optionally Eerie Choir for pause/resume. The remaining rows provide
the populated interface and category diversity. Suggested 20–25 second take:

1. Hold the populated Main Queue view for two seconds.
2. Select `overnight`, open its editor, and show the enabled schedule.
3. Return to Main Queue and import the reserved URL from the clipboard.
4. Show its progress, pause/resume it, and finish on the populated dashboard.

Print the reserved URL for copying:

```sh
python3 scripts/demo.py clipboard
```

The URL is:

```text
https://download.blender.org/durian/trailer/sintel_trailer-720p.mp4
```

It is a 7.6 MB Video download. In Ario, select Main Queue and press `v` to
import the clipboard. This becomes item 51 and is excluded from the reset snapshot.

For headless operation and a separately launched UI:

```sh
python3 scripts/demo.py start --no-ui
python3 scripts/demo.py ui
```

## Stop, reset, and verify

Close the TUI before reset or rebuild. Quitting the demo TUI leaves the demo
daemon running until you stop or reset it.

```sh
python3 scripts/demo.py stop
python3 scripts/demo.py reset
python3 scripts/demo.py start
```

Reset stops owned demo workers, restores the database and payload snapshot,
removes the clipboard take's files, and restores the initial 50 entries.
Prepared transfers are paused in the snapshot; `start` resumes only the two
movie downloads. Stopping clears process-specific aria2 IDs and its session
list while preserving real partial files and their `.aria2` control files.

```sh
python3 scripts/demo.py verify
python3 scripts/demo.py check-sources
python3 -m unittest discover -s scripts/tests -v
```

Verification checks queue counts, seed URLs, category routing, downloaded bytes,
the overnight schedule, and hashes of the eight completed payloads. Reset after
a long take if a movie completes. To smoke-test the URL import API that the
clipboard dialog uses, while the demo is running:

```sh
python3 scripts/demo.py verify --clipboard
python3 scripts/demo.py reset
```

The smoke test imports the reserved URL, waits for real completion, and checks
that it became item 51 in Main Queue as Video. It does not operate the system
clipboard or press keys in the TUI; the actual recording uses `v` as above.

`check-sources` is an optional audit of all 51 URLs. It can report unavailable
queued manuals without affecting the prepared recording state. The preparation
metadata labels each URL as freshly `checked` or `catalog-only`, so cached sizes
are distinguishable from current server responses.

Logs are in `.demo/ario-readme/daemon.log` and
`.demo/ario-readme/live/config/ario/aria2.log`. Fresh source metadata is saved
as `verified-sources.json` beside the database's `live` directory. On any
interrupted command, run `stop` before retrying. No product API or schema changes
are required by this workflow.

## Sources and credits

[`demo/manifest.json`](../demo/manifest.json) records the original direct URL,
source page, creator/project attribution, license and license link, size and
verification date, category, and intended state for every item. It includes
all 50 seed items and the separate clipboard item. Version numbers are pinned
recording fixtures, not recommendations to install those releases.

| Collection | License / source |
| --- | --- |
| Elephants Dream | Blender Foundation; [CC BY 2.5](https://orange.blender.org/). Keep original credits. |
| Sintel and its trailer | Blender Foundation; [CC BY 3.0](https://durian.blender.org/sharing/). Keep original credits. |
| Big Buck Bunny | Blender Foundation; [CC BY 3.0](https://peach.blender.org/about/). Keep original credits. |
| Tears of Steel | Blender Foundation; [CC BY 3.0](https://mango.blender.org/about/). Keep original credits. |
| Retro Tunes | TheFrenchDodo; [CC0](https://opengameart.org/content/retro-tunes). |
| Free Surreal/Dream Music Pack | tricksntraps; [CC0](https://opengameart.org/content/free-surrealdream-music-pack). |
| Music Pack 1 | phoenix1291 / SwissArcadeGameEntertainment; [CC0](https://opengameart.org/content/music-pack-1). |
| HDR lighting environments | Poly Haven and contributing artists; [CC0](https://polyhaven.com/license). |
| GNU manuals | Free Software Foundation; [GNU FDL](https://www.gnu.org/licenses/fdl-1.3.html), including each manual's notices and applicable cover texts. |
| GNU source archives | Free Software Foundation and contributors; GPL licenses retained in the archives. |
| Debian packages | Upstream authors and Debian maintainers; [Debian main](https://www.debian.org/distrib/packages) free software with bundled copyright notices. |
| Git source | Git contributors; [GPL v2](https://git-scm.com/about/free-and-open-source). |
| Python source | Python Software Foundation and contributors; [PSF license and included component notices](https://docs.python.org/3/license.html). |
| Blender distributions | Blender Foundation and contributors; [GNU GPL and included component notices](https://www.blender.org/about/license/). |
| Debian and Alpine images | Official downloadable distributions with component-specific notices: [Debian](https://www.debian.org/CD/), [Alpine](https://alpinelinux.org/about/). |

Keep the downloaded originals and embedded credits intact. This repository
ships the manifest, tooling, and source credits; downloaded third-party content
stays in the ignored demo directory. The GIF shows Ario's interface and filenames.
