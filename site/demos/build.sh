#!/usr/bin/env bash
# Regenerates the docs site's demos (`mise run demos`, from the repo root):
# the recording from the fixtures, then each tape with VHS, then a poster
# PNG per video. Needs vhs and ffmpeg on PATH, and the 0xProto Nerd Font.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release --quiet
cargo run --release --quiet --example demo_recording -- daisy-session.jsonl.gz

for tape in ports charts; do
  vhs "$tape.tape"
  # The poster: the first frame of the video, shown until it plays.
  ffmpeg -loglevel error -y -i "$tape.webm" -frames:v 1 "$tape.png"
done

du -ch daisy-session.jsonl.gz ./*.webm ./*.png | tail -n 1
