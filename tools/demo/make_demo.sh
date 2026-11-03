#!/usr/bin/env bash
# Builds the README's demo GIF from the frames the Flutter harness renders.
#
#     flutter test test/gen_demo.dart     # from app/ — writes /tmp/earshot-demo/frame-*.png
#     tools/demo/make_demo.sh             # from anywhere — writes docs/demo.gif
#
# Two passes rather than one: a GIF carries at most 256 colours, and letting ffmpeg pick them from
# the whole clip (palettegen) instead of from a fixed table is the difference between a readable
# dark green screen and a banded mess. `bayer` dithering is chosen over the default because the
# screen is large flat fields of near-black, where error diffusion crawls between frames.
set -euo pipefail

frames=${1:-/tmp/earshot-demo}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
out="$repo/docs/demo.gif"
width=260
fps=10

[ -e "$frames/frame-0000.png" ] || { echo "no frames in $frames — run the harness first" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

ffmpeg -v error -framerate "$fps" -i "$frames/frame-%04d.png" \
  -vf "scale=$width:-1:flags=lanczos,palettegen=max_colors=96:stats_mode=full" \
  -y "$tmp/palette.png"

ffmpeg -v error -framerate "$fps" -i "$frames/frame-%04d.png" -i "$tmp/palette.png" \
  -lavfi "scale=$width:-1:flags=lanczos[s];[s][1:v]paletteuse=dither=bayer:bayer_scale=3" \
  -loop 0 -y "$out"

echo "$out  $(du -h "$out" | cut -f1)"
