"""Draws the Barra icon: a deep blue squircle with a glass menu bar at the
top and a glass dock at the bottom holding three app dots, the two places
Barra lives on the desktop.

Rendered at 4x and downsampled, so edges are smooth and the area outside the
squircle is fully transparent.

Writes assets/brand/barra-1024.png and assets/brand/barra.ico, and copies the
.ico to the Settings app.

Usage: python scripts/make_barra_icon.py
"""

import shutil
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "assets" / "brand"
SETTINGS_ICON = ROOT / "apps" / "settings" / "src-tauri" / "icons" / "icon.ico"

SS = 4
N = 1024 * SS
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
MARGIN = 72 / 1024
EXPONENT = 5.0

# Gradient from top-left to bottom-right.
TOP = np.array((64, 156, 255), dtype=float)
BOTTOM = np.array((40, 52, 160), dtype=float)


def squircle_mask() -> np.ndarray:
    c = (np.arange(N) + 0.5) / N
    x, y = np.meshgrid(c, c)
    half = 0.5 - MARGIN
    d = (np.abs(x - 0.5) / half) ** EXPONENT + (np.abs(y - 0.5) / half) ** EXPONENT
    return (d <= 1.0).astype(float)


def background() -> Image.Image:
    c = np.linspace(0.0, 1.0, N)
    x, y = np.meshgrid(c, c)
    t = np.clip((x * 0.35 + y * 0.65), 0, 1)[..., None]
    rgb = TOP * (1 - t) + BOTTOM * t
    # A soft light from the top left.
    glow = np.exp(-(((x - 0.25) ** 2 + (y - 0.15) ** 2) / 0.08))[..., None]
    rgb = np.clip(rgb + glow * 40, 0, 255)
    alpha = squircle_mask() * 255
    return Image.fromarray(np.dstack([rgb, alpha]).astype(np.uint8), "RGBA")


def s(v: float) -> int:
    return round(v * SS)


def glass(draw: ImageDraw.ImageDraw, box, radius, fill=(255, 255, 255, 70), edge=(255, 255, 255, 150)):
    draw.rounded_rectangle([s(v) for v in box], radius=s(radius), fill=fill, outline=edge, width=s(4))


def main() -> None:
    icon = background()
    layer = Image.new("RGBA", icon.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    # Menu bar: a thin glass strip near the top with a clock pill on the right.
    glass(draw, (168, 210, 856, 300), 45)
    draw.rounded_rectangle([s(v) for v in (212, 240, 330, 270)], radius=s(15), fill=(255, 255, 255, 200))
    draw.rounded_rectangle([s(v) for v in (700, 240, 812, 270)], radius=s(15), fill=(255, 255, 255, 200))
    # Dock: a wider glass pill at the bottom with three app dots.
    glass(draw, (168, 650, 856, 820), 85)
    colors = ((255, 196, 64), (255, 255, 255), (96, 230, 200))
    for index, color in enumerate(colors):
        cx = 330 + index * 182
        r = 52
        draw.rounded_rectangle([s(cx - r), s(735 - r), s(cx + r), s(735 + r)], radius=s(30), fill=color + (255,))
    # Soft shadow under the glass shapes for depth.
    shadow = layer.split()[3].filter(ImageFilter.GaussianBlur(s(14)))
    dark = Image.new("RGBA", icon.size, (10, 20, 70, 0))
    dark.putalpha(shadow.point(lambda a: a * 0.35))
    icon = Image.alpha_composite(icon, dark.transform(icon.size, Image.AFFINE, (1, 0, 0, 0, 1, -s(10))))
    icon = Image.alpha_composite(icon, layer)
    # Keep everything inside the squircle.
    mask = Image.fromarray((squircle_mask() * 255).astype(np.uint8), "L")
    icon.putalpha(Image.fromarray(np.minimum(np.array(icon.split()[3]), np.array(mask))))
    big = icon.resize((1024, 1024), Image.LANCZOS)
    BRAND.mkdir(parents=True, exist_ok=True)
    big.save(BRAND / "barra-1024.png")
    big.save(BRAND / "barra.ico", sizes=[(n, n) for n in ICO_SIZES])
    shutil.copyfile(BRAND / "barra.ico", SETTINGS_ICON)
    print("ok")


if __name__ == "__main__":
    main()
