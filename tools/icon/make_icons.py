#!/usr/bin/env python3
"""Draws the Earshot app icon and writes every size Android wants.

The mark is a microphone capsule sitting over three concentric arcs: the innermost is the cradle a
microphone actually has, and the two outside it turn the same shape into a signal going out. That
is the whole product in one glyph — a microphone, at a distance.

Run it from anywhere:

    python3 tools/icon/make_icons.py

It rewrites app/android/app/src/main/res/mipmap-*/ and the adaptive-icon layers. Everything is
drawn at 8x and downsampled, because PIL has no anti-aliasing of its own.

Requires Pillow.
"""

from pathlib import Path

from PIL import Image, ImageDraw

# The app's seed colour, so the icon and the UI agree.
GREEN = (61, 220, 151, 255)
# Near-black with a green cast, rather than pure black: pure black looks like a hole on an OLED
# home screen next to other icons.
BACKDROP = (11, 19, 16, 255)

SS = 8  # supersampling factor
BASE = 1024  # the canvas the glyph is designed in

REPO = Path(__file__).resolve().parents[2]
RES = REPO / "app/android/app/src/main/res"

# Android launcher icons, legacy square. dp -> px at each density.
LEGACY = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
# Adaptive icons are 108dp, of which only the middle 72dp is guaranteed to be visible — the
# launcher masks and animates the rest. The glyph is drawn to fit that inner square.
ADAPTIVE = {"mdpi": 108, "hdpi": 162, "xhdpi": 216, "xxhdpi": 324, "xxxhdpi": 432}


def draw_mark(size, inset):
    """The glyph alone, transparent behind it.

    `inset` is how much of the canvas edge to keep clear: 0 fills the canvas (legacy icons, which
    are their own final shape), higher values pull the glyph into the adaptive-icon safe zone.
    """
    canvas = size * SS
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Everything below is expressed against BASE and then scaled, so the design reads the same at
    # every output size.
    scale = canvas / BASE * (1 - 2 * inset)
    ox = canvas / 2
    # The design runs from the top of the capsule (-290) to the outermost arc (+350), so its own
    # midpoint is 30 below the origin. Lifting it by that much centres the *ink*, which is what the
    # eye judges — centring the coordinate system instead leaves the mark looking like it has sunk.
    oy = canvas / 2 - 30 * canvas / BASE * (1 - 2 * inset)

    def px(v):
        return v * scale

    def at(x, y):
        return (ox + px(x), oy + px(y))

    # The capsule. Coordinates are relative to the centre of the whole mark.
    cap_w, cap_top, cap_bot = 190, -290, 10
    d.rounded_rectangle(
        [at(-cap_w / 2, cap_top), at(cap_w / 2, cap_bot)],
        radius=px(cap_w / 2),
        fill=GREEN,
    )

    # Three arcs beneath it, sharing a centre just below the capsule. The first is the cradle; the
    # others are the same gesture continued outward, which is what makes it read as transmission
    # rather than as a plain microphone.
    arc_cx, arc_cy = 0, -40
    for radius, thickness, spread in ((210, 42, 180), (300, 38, 140), (390, 34, 100)):
        start = 90 - spread / 2
        d.arc(
            [
                at(arc_cx - radius, arc_cy - radius),
                at(arc_cx + radius, arc_cy + radius),
            ],
            start=start,
            end=start + spread,
            fill=GREEN,
            width=max(1, int(px(thickness))),
        )

    return img.resize((size, size), Image.LANCZOS)


def legacy_icon(size):
    """Square icon with its own background — what pre-Android-8 launchers draw directly."""
    img = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # A rounded square rather than a full bleed: launchers that do not mask still look deliberate.
    d.rounded_rectangle(
        [0, 0, size * SS - 1, size * SS - 1],
        radius=size * SS * 0.22,
        fill=BACKDROP,
    )
    img = img.resize((size, size), Image.LANCZOS)
    img.alpha_composite(draw_mark(size, inset=0.05))
    return img


def write(path, img):
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)
    print(f"  {path.relative_to(REPO)}  {img.size[0]}x{img.size[1]}")


def main():
    print("legacy launcher icons")
    for density, size in LEGACY.items():
        write(RES / f"mipmap-{density}/ic_launcher.png", legacy_icon(size))

    print("adaptive foreground layers")
    for density, size in ADAPTIVE.items():
        # The mark is 0.76 of the design canvas wide; the safe zone is 72/108 = 0.67 of the icon.
        # An inset of 0.10 puts it at 0.61 — inside the guarantee, with room for the mask.
        write(RES / f"mipmap-{density}/ic_launcher_foreground.png", draw_mark(size, inset=0.10))

    print("store / readme icon")
    write(REPO / "docs/icon.png", legacy_icon(512))

    print("in-app mark (transparent, for the header)")
    write(REPO / "app/assets/icon.png", draw_mark(256, inset=0.04))


if __name__ == "__main__":
    main()
