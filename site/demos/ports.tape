# The Ports tab: the dock coming up (stuck at USB 2 for a moment), a
# device's details, the filter, the help overlay, and ghosts after the
# unplug. Replays daisy-session.jsonl.gz at x2. Run by site/demos/build.sh.
Output ports.webm
Source settings.tape

Hide
Type "../../target/release/plugii replay daisy-session.jsonl.gz --speed 2"
Enter
Sleep 600ms
Show

# Nothing plugged in; at 0:04 the dock arrives, its hub on USB 2 only (red),
# then its USB 3 half links.
Sleep 7s

# Down to the Ethernet adapter behind the dock's hub, and open it.
Type@120ms "jjjjjjjjj"
Sleep 800ms
Enter
Sleep 3.5s
Escape
Sleep 600ms

# Filter the tree.
Type "/"
Sleep 300ms
Type@120ms "hub"
Sleep 600ms
Enter
Sleep 1.5s
Type "n"
Sleep 1.2s
Escape
Sleep 800ms

# The help overlay lists every key.
Type "?"
Sleep 3s
Escape

# The dock leaves at 0:17: its devices stay a while as ghosts; then a charger.
Sleep 6s
Type "q"
Sleep 300ms
