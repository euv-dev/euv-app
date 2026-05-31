"""Generate Android mipmap icons from euv.png."""
from PIL import Image
import os

SRC = r"D:\code\ltpp-docs\src\.vuepress\public\img\euv.png"
OUT = r"C:\Users\14915\Desktop\truri-app\src-tauri\gen\android\app\src\main\res"

im = Image.open(SRC).convert("RGBA")

# Android mipmap sizes per density bucket
# ic_launcher.png = adaptive icon background (108dp @ 1.5x = 163px min, use 108x108 safe)
# ic_launcher_foreground.png = adaptive icon foreground (72dp, use 162x162 for xxxhdpi)
# ic_launcher_round.png = round icon
# Standard sizes per density:
#   mdpi=48, hdpi=72, xhdpi=96, xxhdpi=144, xxxhdpi=192
# Adaptive icon foreground: mdpi=108, hdpi=162, xhdpi=216, xxhdpi=324, xxxhdpi=432
sizes = {
    "mipmap-mdpi": {
        "ic_launcher.png": 48,
        "ic_launcher_round.png": 48,
        "ic_launcher_foreground.png": 108,
    },
    "mipmap-hdpi": {
        "ic_launcher.png": 72,
        "ic_launcher_round.png": 72,
        "ic_launcher_foreground.png": 162,
    },
    "mipmap-xhdpi": {
        "ic_launcher.png": 96,
        "ic_launcher_round.png": 96,
        "ic_launcher_foreground.png": 216,
    },
    "mipmap-xxhdpi": {
        "ic_launcher.png": 144,
        "ic_launcher_round.png": 144,
        "ic_launcher_foreground.png": 324,
    },
    "mipmap-xxxhdpi": {
        "ic_launcher.png": 192,
        "ic_launcher_round.png": 192,
        "ic_launcher_foreground.png": 432,
    },
}

for folder, icons in sizes.items():
    folder_path = os.path.join(OUT, folder)
    os.makedirs(folder_path, exist_ok=True)
    for name, size in icons.items():
        resized = im.resize((size, size), Image.LANCZOS)
        resized.save(os.path.join(folder_path, name), "PNG")
        print(f"  {folder}/{name} ({size}x{size})")

# Also generate a 512x512 play store icon
play_icon = im.resize((512, 512), Image.LANCZOS)
play_icon.save(os.path.join(OUT, "mipmap-xxxhdpi", "ic_launcher_web.png"), "PNG")
print(f"  ic_launcher_web.png (512x512)")

print("Done!")
