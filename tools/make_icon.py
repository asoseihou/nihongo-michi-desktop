"""Draws the app icon (道 on the Michi teal) as a 1024 px PNG.
Run: python tools/make_icon.py && cargo tauri icon src-tauri/icons/source.png
Also writes the small tray icon."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
FONT = "C:/Windows/Fonts/YuGothB.ttc"

def icon(size, radius_frac=0.2):
    im = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    pad = int(size * 0.03)
    d.rounded_rectangle([pad, pad, size - pad, size - pad], radius=int(size * radius_frac), fill=(10, 154, 143, 255))
    f = ImageFont.truetype(FONT, int(size * 0.66))
    d.text((size / 2, size / 2 + size * 0.02), "道", font=f, fill=(255, 255, 255, 255), anchor="mm")
    return im

icon(1024).save(ROOT / "src-tauri/icons/source.png")
icon(64, 0.22).save(ROOT / "src-tauri/icons/tray.png")
print("ok")

# Bundle icons (same set `cargo tauri icon` makes for Windows).
src = Image.open(ROOT / "src-tauri/icons/source.png")
icons = ROOT / "src-tauri/icons"
for name, s in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)]:
    src.resize((s, s), Image.LANCZOS).save(icons / name)
src.save(icons / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
src.resize((512, 512), Image.LANCZOS).save(icons / "icon.icns")
print("bundle icons ok")
