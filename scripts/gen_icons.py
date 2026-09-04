#!/usr/bin/env python3
"""Generate Tauri app icon set from a source PNG logo.

Requires:
  - Pillow (pip install Pillow)
  - ImageMagick (for multi-resolution icon.ico)
  - iconutil (macOS only, for icon.icns)

Usage:
  python3 scripts/gen_icons.py src-tauri/icons/source.png
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

# Tauri square PNG icons
TAURI_PNGS = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
}

# macOS .iconset required sizes
ICNS_SIZES = [
    (16, 1),
    (16, 2),
    (32, 1),
    (32, 2),
    (128, 1),
    (128, 2),
    (256, 1),
    (256, 2),
    (512, 1),
    (512, 2),
]

# Windows ICO member sizes (larger first, PNG-embedded 256 supported on Vista+)
ICO_SIZES = [256, 48, 32, 16]


def make_square(src: Image.Image, size: int, fill_ratio: float = 0.92,
                bg: tuple[int, int, int, int] | None = None,
                corner_radius: float = 0.0) -> Image.Image:
    """Center `src` on a square canvas, preserving aspect ratio.

    `bg` 为画布底色（RGBA）；None 表示透明底（旧行为）。
    `corner_radius` 为圆角比例（0~0.5，相对边长）；>0 且有底色时底色画成圆角矩形，
    圆角外保持透明（alpha 合成，不会污染 Logo 自身的半透明像素）。
    """
    if src.mode != "RGBA":
        src = src.convert("RGBA")

    scale = (size * fill_ratio) / max(src.width, src.height)
    new_w = max(1, int(src.width * scale))
    new_h = max(1, int(src.height * scale))
    resized = src.resize((new_w, new_h), Image.LANCZOS)

    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    if bg is not None:
        if corner_radius > 0:
            layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
            draw = ImageDraw.Draw(layer)
            radius = max(1, int(size * min(corner_radius, 0.5)))
            draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=bg)
            canvas = Image.alpha_composite(canvas, layer)
        else:
            canvas = Image.new("RGBA", (size, size), bg)

    paste_x = (size - new_w) // 2
    paste_y = (size - new_h) // 2
    canvas.paste(resized, (paste_x, paste_y), resized)
    return canvas


def parse_bg(value: str) -> tuple[int, int, int, int] | None:
    """Parse --bg: 'none' = transparent, '#RRGGBB' or 'R,G,B' = opaque color."""
    v = value.strip().lower()
    if v == "none":
        return None
    if v.startswith("#"):
        h = v[1:]
        if len(h) == 6:
            return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), 255)
        raise argparse.ArgumentTypeError(f"invalid hex color: {value}")
    parts = v.split(",")
    if len(parts) in (3, 4):
        nums = [int(p) for p in parts]
        if len(nums) == 3:
            nums.append(255)
        if all(0 <= n <= 255 for n in nums):
            return tuple(nums)  # type: ignore[return-value]
    raise argparse.ArgumentTypeError(f"invalid background color: {value}")


def build_icns(iconset_dir: Path, out_path: Path, src: Image.Image,
               bg: tuple[int, int, int, int] | None, corner_radius: float) -> None:
    """Build icon.icns from a .iconset folder using macOS iconutil.

    Uses a temporary work directory because iconutil writes the .icns next to
    the .iconset; we then move it into the final out_path and clean up.
    """
    import tempfile

    work_dir = tempfile.mkdtemp(prefix="dfy_icns_")
    iconset_path = Path(work_dir) / "icon.iconset"
    iconset_path.mkdir(parents=True)

    for base_size, scale in ICNS_SIZES:
        pixel_size = base_size * scale
        img = make_square(src, pixel_size, bg=bg, corner_radius=corner_radius)
        suffix = f"@{scale}x" if scale > 1 else ""
        img.save(iconset_path / f"icon_{base_size}x{base_size}{suffix}.png")

    subprocess.run(["iconutil", "-c", "icns", str(iconset_path)], check=True)
    produced = Path(work_dir) / "icon.icns"

    if out_path.exists():
        out_path.unlink()
    shutil.move(str(produced), str(out_path))

    shutil.rmtree(work_dir)
    if iconset_dir.exists():
        shutil.rmtree(iconset_dir)


def build_ico(out_path: Path, src: Image.Image,
              bg: tuple[int, int, int, int] | None, corner_radius: float) -> None:
    """Build multi-resolution icon.ico using ImageMagick."""
    if not shutil.which("magick"):
        raise RuntimeError("ImageMagick `magick` command not found in PATH")

    tmp_dir = out_path.parent / ".ico_tmp"
    if tmp_dir.exists():
        shutil.rmtree(tmp_dir)
    tmp_dir.mkdir(parents=True)

    png_paths = []
    for size in ICO_SIZES:
        img = make_square(src, size, bg=bg, corner_radius=corner_radius)
        p = tmp_dir / f"icon_{size}x{size}.png"
        img.save(p)
        png_paths.append(p)

    if out_path.exists():
        out_path.unlink()

    # Largest first tends to produce more compatible .ico files for Windows
    cmd = ["magick"] + [str(p) for p in png_paths] + [str(out_path)]
    subprocess.run(cmd, check=True)

    shutil.rmtree(tmp_dir)


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate Tauri icon set from a PNG logo.")
    parser.add_argument(
        "source",
        nargs="?",
        default="src-tauri/icons/source.png",
        help="Path to source PNG logo (default: src-tauri/icons/source.png)",
    )
    parser.add_argument(
        "--out-dir",
        default="src-tauri/icons",
        help="Output directory for generated icons (default: src-tauri/icons)",
    )
    parser.add_argument(
        "--bg",
        type=parse_bg,
        default="#FFFFFF",
        help="Canvas background: hex '#RRGGBB', 'R,G,B', or 'none' for transparent (default: white)",
    )
    parser.add_argument(
        "--corner-radius",
        type=float,
        default=0.2,
        help="Rounded-corner ratio of the background (0~0.5, 0 disables; default: 0.2)",
    )
    args = parser.parse_args()

    src_path = Path(args.source)
    out_dir = Path(args.out_dir)
    if not src_path.is_file():
        print(f"Source not found: {src_path}", file=sys.stderr)
        return 1
    out_dir.mkdir(parents=True, exist_ok=True)

    src = Image.open(src_path)

    # Tauri square PNGs
    for filename, size in TAURI_PNGS.items():
        make_square(src, size, bg=args.bg, corner_radius=args.corner_radius).save(out_dir / filename)

    # Windows ICO
    build_ico(out_dir / "icon.ico", src, args.bg, args.corner_radius)

    # macOS ICNS (only if iconutil is available; harmless on non-macOS dev machines)
    if shutil.which("iconutil"):
        build_icns(out_dir / "icon.iconset", out_dir / "icon.icns", src, args.bg, args.corner_radius)
    else:
        print("iconutil not found; skipping icon.icns (macOS only)")

    print(f"Generated Tauri icons in {out_dir.resolve()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
