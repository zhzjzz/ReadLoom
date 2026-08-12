from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
ASSET_DIR = ROOT / "crates" / "readloom-slint" / "assets"
CANVAS_SIZE = 1024
SCALE = CANVAS_SIZE / 512


def scaled(value: int) -> int:
    return round(value * SCALE)


def build_icon() -> Image.Image:
    image = Image.new("RGBA", (CANVAS_SIZE, CANVAS_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle(
        (scaled(80), scaled(80), scaled(432), scaled(432)),
        radius=scaled(70),
        fill=(31, 34, 40, 255),
    )
    font_path = Path("C:/Windows/Fonts/seguisb.ttf")
    font = ImageFont.truetype(str(font_path), scaled(190))
    bounds = draw.textbbox((0, 0), "R", font=font)
    text_width = bounds[2] - bounds[0]
    text_height = bounds[3] - bounds[1]
    text_x = (CANVAS_SIZE - text_width) / 2 - bounds[0]
    text_y = scaled(256) - text_height / 2 - bounds[1] + scaled(4)
    draw.text((text_x, text_y), "R", font=font, fill=(255, 255, 255, 255))
    return image


def main() -> None:
    ASSET_DIR.mkdir(parents=True, exist_ok=True)
    source = build_icon()
    png = source.resize((512, 512), Image.Resampling.LANCZOS)
    png.save(ASSET_DIR / "app-icon.png", optimize=True)
    source.save(
        ASSET_DIR / "app-icon.ico",
        format="ICO",
        sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


if __name__ == "__main__":
    main()
