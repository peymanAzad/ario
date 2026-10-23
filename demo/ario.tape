# Run with python3 scripts/record_vhs.py for the current theme and isolated clipboard.
# The wrapper exports ario-readme.gif at 15 FPS, then removes this intermediate.
Output .demo/ario-readme/recordings/.capture.mp4
Output .demo/ario-readme/recordings/ario.txt
Set Shell bash
Set FontSize 20
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set Width 1440
Set Height 900
Set Framerate 15
Set Theme "Catppuccin Frappe"
Set Padding 16
Set WindowBar Colorful
Set CursorBlink false
Set TypingSpeed 90ms
Set WaitTimeout 60s

Require python3

Hide
Copy "https://download.blender.org/durian/trailer/sintel_trailer-720p.mp4"
Type@1ms "python3 scripts/demo.py ui"
Enter
Wait+Screen /Main Queue/
Type "1"
Down
Sleep 500ms
Type "3"
Sleep 500ms

Show
Sleep 3s
Screenshot .demo/ario-readme/recordings/main.png
Sleep 500ms

# Show the selection color in each pane and the category filter.
Type "1"
Sleep 2s
Screenshot .demo/ario-readme/recordings/queues.png
Sleep 500ms
Type "2"
Down
Sleep 3s
Screenshot .demo/ario-readme/recordings/categories.png
Sleep 500ms
Up
Type "3"
Sleep 1s
Type "p"
Sleep 3s
Type "r"
Sleep 3s

# Create a real queue; linger on its common options and scheduler tab.
Type "1n"
Wait+Screen /Create Queue/
Sleep 2s
Enter
Type "weekend projects"
Enter
Down
Right
Sleep 4s
Screenshot .demo/ario-readme/recordings/create-queue.png
Sleep 500ms
Down@400ms 3
Sleep 2s
Tab
Sleep 2s
Right
Down@400ms 2
Right@200ms 5
Space
Right
Space
Sleep 5s
Screenshot .demo/ario-readme/recordings/create-schedule.png
Sleep 500ms
Type "s"
Sleep 3s

# Show the prepared overnight schedule and its list of queued downloads.
Down@300ms 2
Enter
Tab
Sleep 5s
Screenshot .demo/ario-readme/recordings/schedule.png
Sleep 500ms
Tab
Sleep 4s
Escape
Up@300ms 2
Sleep 1s

# Clipboard import: queue choice, URL selection, and Fine Tuning options.
Type "v"
Wait+Screen /Import from Clipboard/
Sleep 5s
Right
Sleep 2s
Left
Space
Sleep 1s
Space
Tab
Sleep 4s
Right
Down
Right
Sleep 4s
Screenshot .demo/ario-readme/recordings/import-options.png
Sleep 500ms
Tab
Sleep 4s
Screenshot .demo/ario-readme/recordings/clipboard.png
Sleep 500ms
Type "s"
Sleep 1s
Type "3"
Down@20ms 20
Hide
Wait+Screen /sintel_trailer-720p\.mp4[^\n]*[0-9]+\.[0-9]+%[^\n]*[0-9]+\.[0-9]+ (KB|MB)\/s/
Show
Sleep 8s
Screenshot .demo/ario-readme/recordings/download.png
Sleep 500ms

Hide
Ctrl+c
Sleep 1s
