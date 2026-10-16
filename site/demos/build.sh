#!/usr/bin/env bash
# Regenerates the docs site's demos (`mise run demos`, from the repo root):
# the recording from the fixtures, then each tape with VHS, then a poster
# PNG per video, and the README's GIF and still. Needs vhs and ffmpeg on PATH, and the 0xProto Nerd Font.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release --quiet
cargo run --release --quiet --example demo_recording -- daisy-session.jsonl.gz

for tape in ports charts; do
  vhs "$tape.tape"
  # The poster: the first frame of the video, shown until it plays.
  ffmpeg -loglevel error -y -i "$tape.webm" -frames:v 1 "$tape.png"
done

# The README's media (GitHub won't autoplay a video there): the Ports demo
# as a GIF, and a still of the Charts tab once both chargers are in.
media=../../docs/media
mkdir -p "$media"
ffmpeg -loglevel error -y -i ports.webm -vf "fps=8,scale=1000:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=32:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" "$media/ports.gif"
ffmpeg -loglevel error -y -ss 12 -i charts.webm -frames:v 1 -vf scale=1000:-1:flags=lanczos "$media/charts.png"

du -ch daisy-session.jsonl.gz ./*.webm ./*.png "$media"/* | tail -n 1
