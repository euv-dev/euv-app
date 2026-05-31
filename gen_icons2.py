"""Regenerate proper multi-size ICO and ICNS from euv.png."""
from PIL import Image
import os
import struct
import io

SRC = r"D:\code\ltpp-docs\src\.vuepress\public\img\euv.png"
OUT = r"C:\Users\14915\Desktop\truri-app\src-tauri\icons"

im = Image.open(SRC).convert("RGBA")

# --- Proper ICO with multiple sizes ---
# ICO format: ICONDIR (6 bytes) + ICONDIRENTRY (16 bytes per image) + image data
# Each image data is a BITMAPINFOHEADER + XOR mask + AND mask
# Simpler approach: use PNG-in-ICO (Windows Vista+ supports this)

ico_sizes = [16, 24, 32, 48, 64, 96, 128, 256]
ico_entries = []

for s in ico_sizes:
    resized = im.resize((s, s), Image.LANCZOS)
    buf = io.BytesIO()
    resized.save(buf, format="PNG")
    ico_entries.append((s, buf.getvalue()))

# Build ICO file
# ICONDIR: reserved(2) + type(2) + count(2)
header = struct.pack("<HHH", 0, 1, len(ico_entries))

# Calculate offset: 6 (header) + 16 * count (directory entries)
offset = 6 + 16 * len(ico_entries)
directory = b""
data_blocks = b""

for size, png_data in ico_entries:
    # ICONDIRENTRY: width(1) + height(1) + colors(1) + reserved(1) + planes(2) + bpp(2) + size(4) + offset(4)
    width = size if size < 256 else 0
    height = size if size < 256 else 0
    directory += struct.pack("<BBBBHHII", width, height, 0, 0, 0, 0, len(png_data), offset)
    data_blocks += png_data
    offset += len(png_data)

ico_path = os.path.join(OUT, "icon.ico")
with open(ico_path, "wb") as f:
    f.write(header + directory + data_blocks)

print(f"icon.ico: {os.path.getsize(ico_path)} bytes ({len(ico_sizes)} sizes: {ico_sizes})")

# --- Regenerate ICNS ---
# ICNS format: header(8) + icon data blocks
# Each block: 4-byte type + 4-byte length + data

def icns_png_type(size_px, scale=1):
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

icns_specs = [
    (16, 1), (16, 2),
    (32, 1), (32, 2),
    (64, 1), (64, 2),
    (128, 1), (128, 2),
    (256, 1), (256, 2),
    (512, 1), (512, 2),
    (1024, 1),
]

icns_entries = []
for px, scale in icns_specs:
    actual = px * scale
    code = icns_png_type(px, scale)
    if code is None:
        continue
    resized = im.resize((actual, actual), Image.LANCZOS)
    buf = io.BytesIO()
    resized.save(buf, format="PNG")
    icns_entries.append((code, buf.getvalue()))

icns_path = os.path.join(OUT, "icon.icns")
with open(icns_path, "wb") as f:
    data_blocks = b""
    for code, png_data in icns_entries:
        block_type = code.encode("ascii")
        block_len = 8 + len(png_data)
        data_blocks += struct.pack(">4sI", block_type, block_len) + png_data
    total_len = 8 + len(data_blocks)
    f.write(struct.pack(">4sI", b"icns", total_len))
    f.write(data_blocks)

print(f"icon.icns: {os.path.getsize(icns_path)} bytes ({len(icns_entries)} blocks)")
print("Done!")
