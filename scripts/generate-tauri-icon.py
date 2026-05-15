#!/usr/bin/env python3
from pathlib import Path
import shutil
import subprocess

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
ICONS_DIR = ROOT / "apps" / "tauri-ui" / "src-tauri" / "icons"
PNG_PATH = ICONS_DIR / "icon.png"
ICO_PATH = ICONS_DIR / "icon.ico"
ICONSET_DIR = ICONS_DIR / "icon.iconset"
ICNS_PATH = ICONS_DIR / "icon.icns"


def build_png() -> None:
    ICONS_DIR.mkdir(parents=True, exist_ok=True)

    size = 1024
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, size, size), fill=(255, 255, 255, 255))

    mask = Image.new("L", (size, size), 0)
    mask_draw = ImageDraw.Draw(mask)
    mask_draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=200, fill=255)
    image.putalpha(mask)

    outer = (120, 120, 904, 904)
    inner = (240, 240, 784, 784)
    start = (168, 94, 255, 255)
    end = (255, 116, 193, 255)
    steps = 60
    for i in range(steps):
        t = i / (steps - 1)
        r = int(start[0] + (end[0] - start[0]) * t)
        g = int(start[1] + (end[1] - start[1]) * t)
        b = int(start[2] + (end[2] - start[2]) * t)
        x0 = int(outer[0] + (inner[0] - outer[0]) * t)
        y0 = int(outer[1] + (inner[1] - outer[1]) * t)
        x1 = int(outer[2] + (inner[2] - outer[2]) * t)
        y1 = int(outer[3] + (inner[3] - outer[3]) * t)
        draw.ellipse((x0, y0, x1, y1), fill=(r, g, b, 255))

    draw.ellipse((300, 300, 724, 724), fill=(66, 40, 92, 255))

    font_path = Path("/System/Library/Fonts/Supplemental/Arial Bold.ttf")
    if font_path.exists():
        font = ImageFont.truetype(str(font_path), 276)
    else:
        font = ImageFont.load_default()

    text = "px"
    bbox = draw.textbbox((0, 0), text, font=font)
    text_width = bbox[2] - bbox[0]
    text_height = bbox[3] - bbox[1]
    text_x = (size - text_width) // 2
    text_y = (size - text_height) // 2 - 68
    draw.text((text_x, text_y), text, font=font, fill=(236, 243, 255, 255))

    image.save(PNG_PATH, "PNG")
    image.save(ICO_PATH, sizes=[(256, 256), (128, 128), (64, 64), (32, 32), (16, 16)])


def build_icns() -> None:
    if ICONSET_DIR.exists():
        shutil.rmtree(ICONSET_DIR)
    ICONSET_DIR.mkdir(parents=True, exist_ok=True)

    for size in [16, 32, 128, 256, 512]:
        out = ICONSET_DIR / f"icon_{size}x{size}.png"
        out_2x = ICONSET_DIR / f"icon_{size}x{size}@2x.png"
        subprocess.run(["sips", "-z", str(size), str(size), str(PNG_PATH), "--out", str(out)], check=True)
        subprocess.run(
            ["sips", "-z", str(size * 2), str(size * 2), str(PNG_PATH), "--out", str(out_2x)],
            check=True,
        )

    if ICNS_PATH.exists():
        ICNS_PATH.unlink()
    subprocess.run(["iconutil", "-c", "icns", str(ICONSET_DIR), "-o", str(ICNS_PATH)], check=True)
    shutil.rmtree(ICONSET_DIR)


def main() -> None:
    build_png()
    build_icns()
    print(f"generated: {PNG_PATH}")
    print(f"generated: {ICO_PATH}")
    print(f"generated: {ICNS_PATH}")


if __name__ == "__main__":
    main()
