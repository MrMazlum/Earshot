#!/usr/bin/env bash
# Builds the README's demo GIF, and the MP4 the social posts use, from the frames the Flutter
# harness renders.
#
#     flutter test test/gen_demo.dart     # from app/ — writes /tmp/earshot-demo/frame-*.png
#     tools/demo/make_demo.sh             # from anywhere — writes docs/demo.gif and docs/demo.mp4
#
# The harness renders at the phone's real density (1221x2712), so both outputs scale DOWN. The GIF
# was 260 px wide until 2026-11-05, which Reddit's phone app upscaled to screen width — soft
# picture, broken text, on the device most people read on. 420 px is the width at which the small
# monospace labels survive.
#
# 🔴 The MP4 is what goes on Reddit, not the GIF. Reddit re-encodes an uploaded GIF; a video post
# keeps the pixels and plays inline.
#
# Two passes rather than one: a GIF carries at most 256 colours, and letting ffmpeg pick them from
# the whole clip (palettegen) instead of from a fixed table is the difference between a readable
# dark green screen and a banded mess. `bayer` dithering is chosen over the default because the
# screen is large flat fields of near-black, where error diffusion crawls between frames.
set -euo pipefail

frames=${1:-/tmp/earshot-demo}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
out="$repo/docs/demo.gif"
width=420
fps=10

[ -e "$frames/frame-0000.png" ] || { echo "no frames in $frames — run the harness first" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

ffmpeg -v error -framerate "$fps" -i "$frames/frame-%04d.png" \
  -vf "scale=$width:-1:flags=lanczos,palettegen=max_colors=256:stats_mode=full" \
  -y "$tmp/palette.png"

ffmpeg -v error -framerate "$fps" -i "$frames/frame-%04d.png" -i "$tmp/palette.png" \
  -lavfi "scale=$width:-1:flags=lanczos[s];[s][1:v]paletteuse=dither=bayer:bayer_scale=3" \
  -loop 0 -y "$out"

echo "$out  $(du -h "$out" | cut -f1)"

# The MP4: 720 px wide, h264 in the profile every phone and every browser plays, and 30 fps because
# some players stutter on a 10 fps video. The source is still 10 fps; the extra frames are copies.
mp4="$repo/docs/demo.mp4"
ffmpeg -v error -framerate "$fps" -i "$frames/frame-%04d.png" \
  -vf "scale=720:-2:flags=lanczos,fps=30" \
  -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -movflags +faststart \
  -y "$mp4"

echo "$mp4  $(du -h "$mp4" | cut -f1)"
