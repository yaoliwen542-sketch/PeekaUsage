# 生成 PeekaUsage 全套图标文件
# - Windows: PNG 各尺寸 + icon.ico（多尺寸）
# - macOS: icon.icns（手工打包 PNG 条目，含安全边距）+ icon.iconset
# - <=48px 使用小尺寸强化变体（更粗笔画）
import struct
from pathlib import Path

from PIL import Image

BASE = Path(r"D:\Project\PeekaUsage\.logo-lab")
ICONS = Path(r"D:\Project\PeekaUsage\src-tauri\icons")

master = Image.open(BASE / "master-1024.png").convert("RGBA")
master_small = Image.open(BASE / "master-small-1024.png").convert("RGBA")


def pick(size: int) -> Image.Image:
    src = master_small if size <= 48 else master
    return src.resize((size, size), Image.LANCZOS)


# ---------- Windows / 通用 PNG ----------
png_targets = {
    "32x32.png": 32,
    "64x64.png": 64,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 1024,
    "StoreLogo.png": 50,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
}
for name, size in png_targets.items():
    pick(size).save(ICONS / name)
    print("wrote", name, size)

# ---------- icon.ico（16~256 多尺寸，256 用 PNG 压缩） ----------
# 注意：必须用「最大帧 + append_images 其余帧」且不传 sizes 参数，
# 让每帧按自身尺寸写入；传 sizes 会导致 Pillow 只写入一帧
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
frames = [pick(s) for s in ico_sizes]
frames[-1].save(
    ICONS / "icon.ico",
    format="ICO",
    append_images=frames[:-1],
)
print("wrote icon.ico", ico_sizes)

# ---------- macOS iconset（含 824/1024 安全边距） ----------
MAC_SCALE = 824 / 1024
iconset = ICONS / "icon.iconset"
iconset.mkdir(exist_ok=True)


def mac_render(size: int) -> Image.Image:
    tile = pick(size).resize((round(size * MAC_SCALE),) * 2, Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    off = (size - tile.width) // 2
    canvas.alpha_composite(tile, (off, off))
    return canvas


iconset_files = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}
for name, size in iconset_files.items():
    mac_render(size).save(iconset / name)
print("wrote icon.iconset x", len(iconset_files))

# ---------- icon.icns（手工打包：ic07..ic14 PNG 条目） ----------
import io

icns_entries = {
    b"ic07": 128,   # 128x128 PNG
    b"ic08": 256,   # 256x256 PNG
    b"ic09": 512,   # 512x512 PNG
    b"ic10": 1024,  # 1024x1024 PNG（512@2x）
    b"ic11": 64,    # 32@2x
    b"ic12": 64,    # 64x64
    b"ic13": 512,   # 256@2x
    b"ic14": 1024,  # 512@2x
}
# 去重：同尺寸同图只编码一次
png_cache: dict[int, bytes] = {}
for size in set(icns_entries.values()):
    buf = io.BytesIO()
    mac_render(size).save(buf, format="PNG")
    png_cache[size] = buf.getvalue()

blob = b""
for fourcc, size in icns_entries.items():
    data = png_cache[size]
    blob += fourcc + struct.pack(">I", len(data) + 8) + data
with open(ICONS / "icon.icns", "wb") as f:
    f.write(b"icns" + struct.pack(">I", len(blob) + 8) + blob)
print("wrote icon.icns", len(blob) + 8, "bytes")
