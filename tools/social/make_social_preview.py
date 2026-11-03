#!/usr/bin/env python3
"""Draws the repository's social preview card — the picture GitHub, Slack, Discord and every
link unfurler show instead of the page.

    python3 tools/social/make_social_preview.py

Writes docs/social-preview.png at 1280x640, which is the size GitHub asks for. The card is
uploaded by hand under Settings -> General -> Social preview; there is no API for it, so the
file lives here and the upload is a one-off.

It is drawn rather than designed in an editor for the same reason the icon is: the repository
should carry the recipe, so the next person to change the strapline changes one line of Python
instead of hunting for a layered file nobody kept.

Requires Pillow. Uses the app's own faces — JetBrains Mono for the small type, as on the panel
screen — so the card and the app look like one thing.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parents[2]
FONTS = REPO / "app/assets/fonts"
ICON = REPO / "docs/icon.png"
OUT = REPO / "docs/social-preview.png"

W, H = 1280, 640

# The Signal palette, from app/lib/theme.dart. Kept in sync by hand; there are four values.
BACKDROP = (11, 19, 16)
INK = (232, 242, 237)
INK_DIM = (138, 163, 153)
LINE = (32, 48, 41)
GREEN = (61, 220, 151)

# GitHub crops the card on some surfaces and letterboxes it on others. Everything that has to be
# read stays inside this margin.
PAD = 96


def font(name: str, size: int) -> ImageFont.FreeTypeFont:
    path = FONTS / name
    if not path.exists():
        raise SystemExit(f"missing font: {path}")
    return ImageFont.truetype(str(path), size)


def tracked(draw: ImageDraw.ImageDraw, xy, text, fnt, fill, tracking: int) -> int:
    """Draws letterspaced text, which Pillow has no setting for, and returns the width it took.

    The wordmark is spaced like the one on the app's own header, where the width is what makes
    six letters read as a label rather than a word.
    """
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + tracking
    return int(x - tracking - xy[0])


def main() -> None:
    card = Image.new("RGB", (W, H), BACKDROP)
    draw = ImageDraw.Draw(card)

    # The mark, left, vertically centred on the block of type beside it.
    icon = Image.open(ICON).convert("RGBA").resize((248, 248), Image.LANCZOS)
    icon_y = (H - icon.height) // 2
    card.paste(icon, (PAD, icon_y), icon)

    left = PAD + icon.width + 76

    name = font("JetBrainsMonoNL-Bold.ttf", 78)
    line = font("JetBrainsMonoNL-Regular.ttf", 34)
    small = font("JetBrainsMonoNL-Regular.ttf", 25)

    y = 180
    tracked(draw, (left, y), "EARSHOT", name, INK, 10)

    y += 124
    draw.text((left, y), "your phone, as your pc's", font=line, fill=INK_DIM)
    y += 48
    draw.text((left, y), "microphone — over wi-fi", font=line, fill=INK_DIM)

    # The hairline and the platform line, which is the one fact a reader wants before clicking.
    y += 76
    draw.line([(left, y), (W - PAD, y)], fill=LINE, width=2)

    y += 30
    draw.text((left, y), "windows · macos · linux · android", font=small, fill=INK_DIM)

    # One green square, the same one the app's header uses for "ready". The only colour on the
    # card, so it means something.
    draw.rectangle([left - 40, y + 9, left - 40 + 14, y + 23], fill=GREEN)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    card.save(OUT, optimize=True)
    print(f"{OUT}  {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
