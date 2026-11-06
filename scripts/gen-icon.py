# 生成应用图标（`assets/app-icon.png` 与各平台的 `.ico`）。
#
# 为什么程序化绘制而不是引入一张设计稿：
# 1. 图标就是界面里的品牌标（盾牌 + 对勾），用同一套几何参数画出来**天然一致**；
# 2. 不引入二进制设计文件，改一次参数就能重新生成；
# 3. 不依赖 ImageMagick / Inkscape —— 这台机器上没有。
#
# 用法：
#   python scripts/gen-icon.py
#
# 产物（进版本库，发布时直接取用）：
#   assets/app-icon.png       1024×1024，给 macOS / Linux 与文档用
#   assets/app-icon.ico       多尺寸，给 Windows
#   assets/app-icon-256.png   常用尺寸，给 README

from __future__ import annotations

import struct
import zlib
from pathlib import Path

from PIL import Image, ImageDraw

# ── 品牌色（与 `tokens.rs` 的 LIGHT.accent 一致）────────────────
ACCENT = (29, 78, 216, 255)      # #1D4ED8
ON_ACCENT = (255, 255, 255, 255)  # #FFFFFF
BG_TOP = (37, 99, 235, 255)       # #2563EB
BG_BOTTOM = (29, 78, 216, 255)    # #1D4ED8

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets"
SIZE = 1024


def shield_points(w: int, h: int) -> list[tuple[float, float]]:
    """盾牌轮廓（归一化后按画布缩放）。

    形状取自界面里的 `shield-check` 图标：上宽下尖，两肩圆润。
    用二次贝塞尔采样成折线 —— Pillow 没有贝塞尔。
    """
    def bezier(p0, p1, p2, steps=24):
        return [
            (
                (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t**2 * p2[0],
                (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t**2 * p2[1],
            )
            for t in (i / steps for i in range(steps + 1))
        ]

    # 归一化坐标（0..1），y 向下。
    #
    # 形状要点：顶部是一条**平直略拱**的边（不是凹陷），两侧竖直下落，
    # 下半收成尖。第一版把顶部画成了三个贝塞尔拼接，控制点高度不一致，
    # 结果中间凹下去一个尖角 —— 生成出来才发现。
    left, right = 0.20, 0.80
    shoulder_y = 0.20
    waist_y = 0.52
    tip_y = 0.90

    pts = []
    # 顶边：从左肩微微拱到右肩（抬高约 4%）。
    pts += bezier((left, shoulder_y), (0.50, shoulder_y - 0.075), (right, shoulder_y), steps=40)
    # 右肩到腰（竖直略内收）。
    pts += bezier((right, shoulder_y), (right, waist_y - 0.08), (0.66, waist_y), steps=32)
    # 腰到尖。
    pts += bezier((0.66, waist_y), (0.58, tip_y - 0.10), (0.50, tip_y), steps=32)
    # 尖到左腰。
    pts += bezier((0.50, tip_y), (0.42, tip_y - 0.10), (0.34, waist_y), steps=32)
    # 左腰回到左肩。
    pts += bezier((0.34, waist_y), (left, waist_y - 0.08), (left, shoulder_y), steps=32)
    return [(x * w, y * h) for x, y in pts]


def make_icon(size: int = SIZE) -> Image.Image:
    """画一张图标。"""
    # 超采样 4 倍再缩小：Pillow 的折线抗锯齿很差，这是最省事的办法。
    scale = 4
    big = size * scale
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # ① 圆角方形底（垂直渐变）。
    radius = int(big * 0.22)
    gradient = Image.new("RGBA", (big, big))
    gdraw = ImageDraw.Draw(gradient)
    for y in range(big):
        t = y / max(big - 1, 1)
        colour = tuple(int(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3)) + (255,)
        gdraw.line([(0, y), (big, y)], fill=colour)
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, big - 1, big - 1], radius=radius, fill=255)
    img.paste(gradient, (0, 0), mask)

    # ② 盾牌（白色）。
    shield = shield_points(big, big)
    draw.polygon(shield, fill=ON_ACCENT)

    # ③ 对勾（用强调色挖出来）。
    # 对勾必须**完整落在盾牌内部**。
    #
    # 前两版都错在这：第一版太小偏低，第二版太大直接从盾牌下沿伸了出去。
    # 盾牌的"安全区"大约是 x∈[0.30, 0.70]、y∈[0.28, 0.62]（下半收尖，别往下压）。
    stroke = int(big * 0.062)
    check = [
        (big * 0.37, big * 0.44),
        (big * 0.45, big * 0.54),
        (big * 0.64, big * 0.33),
    ]
    draw.line(check, fill=ACCENT, width=stroke, joint="curve")
    # 线端补圆：`line` 的端点是方的，加两个圆点看起来才像 `stroke-linecap: round`。
    for x, y in (check[0], check[-1]):
        r = stroke // 2
        draw.ellipse([x - r, y - r, x + r, y + r], fill=ACCENT)

    return img.resize((size, size), Image.LANCZOS)


def write_ico(img: Image.Image, path: Path, sizes: list[int]) -> None:
    """写一个多尺寸 .ico。

    Pillow 自带的 `save(format="ICO", sizes=...)` 会自己缩放，但它对
    非正方形/超采样图偶尔写出坏目录 —— 这里手动构造，顺便保证
    **每个尺寸都是从 1024 高质量缩下来的**，而不是让 Pillow 用最近邻。
    """
    entries = []
    for size in sorted(sizes):
        frame = img.resize((size, size), Image.LANCZOS)
        # ICO 里存 PNG 数据（Vista+ 支持，体积小、质量好）。
        import io

        buffer = io.BytesIO()
        frame.save(buffer, format="PNG", optimize=True)
        entries.append((size, buffer.getvalue()))

    header = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    directory = b""
    for size, data in entries:
        # 256 在 ICO 里用 0 表示。
        dimension = 0 if size >= 256 else size
        directory += struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(data), offset)
        offset += len(data)

    with path.open("wb") as handle:
        handle.write(header)
        handle.write(directory)
        for _, data in entries:
            handle.write(data)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    icon = make_icon(SIZE)
    icon.save(OUT / "app-icon.png", optimize=True)
    icon.resize((256, 256), Image.LANCZOS).save(OUT / "app-icon-256.png", optimize=True)
    write_ico(icon, OUT / "app-icon.ico", [16, 24, 32, 48, 64, 128, 256])
    print(f"生成完成：{OUT}")
    for name in ("app-icon.png", "app-icon-256.png", "app-icon.ico"):
        print(f"  {name:22} {(OUT / name).stat().st_size:>8} bytes")


if __name__ == "__main__":
    main()
