# The Charts and Events tabs over the whole session at x4: the dock's
# link strip (stuck, then 5G), its URB/s while it copies, power and CPU
# with plug markers, and the charger table. Run by site/demos/build.sh.
Output charts.webm
Source settings.tape

Hide
Type "../../target/release/plugii replay daisy-session.jsonl.gz --speed 4"
Enter
Sleep 600ms
Type "2"
Sleep 300ms
Show

Sleep 18s

# Everything that happened, as events.
Type "3"
Sleep 3.5s
Type "q"
Sleep 300ms
