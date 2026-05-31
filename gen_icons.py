#!/usr/bin/env python3
"""Generate Tauri icons from source image."""
from PIL import Image
import os
import struct
import zlib

SRC = r"D:\code\ltpp-docs\src\.vuepress\public\img\euv.png"
OUT = r"C:\Users\14915\Desktop\truri-app\src-tauri\icons"

im = Image.open(SRC).convert("RGBA")
print(f"Source: {im.size[0]}x{im.size[1]}")

# --- PNG icons at various sizes ---
png_sizes = {
    "32x32.png": (32, 32),
    "128x128.png": (128, 128),
    "128x128@2x.png": (256, 256),
    "icon.png": (512, 512),
    "Square30x30Logo.png": (30, 30),
    "Square44x44Logo.png": (44, 44),
    "Square71x71Logo.png": (71, 71),
    "Square89x89Logo.png": (89, 89),
    "Square107x107Logo.png": (107, 107),
    "Square142x142Logo.png": (142, 142),
    "Square150x150Logo.png": (150, 150),
    "Square284x284Logo.png": (284, 284),
    "Square310x310Logo.png": (310, 310),
    "StoreLogo.png": (50, 50),
}

for name, (w, h) in png_sizes.items():
    resized = im.resize((w, h), Image.LANCZOS)
    resized.save(os.path.join(OUT, name), "PNG")
    print(f"  {name} ({w}x{h})")

# --- Windows .ico ---
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
ico_images = []
for s in ico_sizes:
    ico_images.append(im.resize((s, s), Image.LANCZOS))

ico_path = os.path.join(OUT, "icon.ico")
ico_images[0].save(
    ico_path,
    format="ICO",
    sizes=[(s, s) for s in ico_sizes],
    append_images=ico_images[1:],
)
print(f"  icon.ico (multi-size)")

# --- macOS .icns ---
# Build a minimal valid icns with the needed icon family blocks.
# We include 16, 32, 64, 128, 256, 512, 1024 (1x and 2x where applicable).
# icns format: header(8) + icon data blocks.
# Each block: 4-byte type + 4-byte length + data.
# PNG types for retina: 'ic07'..'ic14' etc. We use PNG for all for simplicity.

def icns_png_type(size_px, scale=1):
    """Return the OSType code string for a given pixel size and scale."""
    key = (size_px, scale)
    table = {
        (16, 1): "icp4",   (16, 2): "ic11",
        (32, 1): "icp5",   (32, 2): "ic12",
        (64, 1): "icp6",   (64, 2): "ic13",
        (128, 1): "ic07",  (128, 2): "ic14",
        (256, 1): "ic08",  (256, 2): "ic1a",
        (512, 1): "ic09",  (512, 2): "ic1b",
        (1024, 1): "ic10",
    }
    return table.get(key)

def img_to_png_bytes(img):
    import io
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()

icns_entries = []
icns_specs = [
    (16, 1), (16, 2),
    (32, 1), (32, 2),
    (64, 1), (64, 2),
    (128, 1), (128, 2),
    (256, 1), (256, 2),
    (512, 1), (512, 2),
    (1024, 1),
]

for px, scale in icns_specs:
    actual = px * scale
    code = icns_png_type(px, scale)
    if code is None:
        continue
    resized = im.resize((actual, actual), Image.LANCZOS)
    png_data = img_to_png_bytes(resized)
    icns_entries.append((code, png_data))
    print(f"  icns block {code} ({actual}x{actual})")

# Write icns file
with open(os.path.join(OUT, "icon.icns"), "wb") as f:
    # header
    header = struct.pack(">4sI", b"icns", 0)  # placeholder total length
    data_blocks = b""
    for code, png_data in icns_entries:
        block_type = code.encode("ascii")
        block_len = 8 + len(png_data)
        data_blocks += struct.pack(">4sI", block_type, block_len) + png_data
    total_len = 8 + len(data_blocks)
    f.write(struct.pack(">4sI", b"icns", total_len))
    f.write(data_blocks)

print(f"  icns written, total {total_len} bytes")
print("Done!")
