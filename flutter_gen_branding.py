#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.8"
# dependencies = [
#     "pillow",
#     "pyyaml",
# ]
# ///
"""Generate branding images with text for light and dark themes."""

import argparse
import logging
import sys
from pathlib import Path
from typing import Optional

import yaml
from PIL import Image, ImageDraw, ImageFont

logging.basicConfig(level=logging.INFO, format='%(levelname)s: %(message)s')
logger = logging.getLogger(__name__)


def load_pubspec(pubspec_path: Path) -> dict:
    """Load and parse pubspec.yaml file."""
    try:
        with pubspec_path.open('r', encoding='utf-8') as f:
            return yaml.safe_load(f)
    except Exception as e:
        logger.error(f"Failed to read {pubspec_path}: {e}")
        sys.exit(1)


def get_font(font_size: int, font_family: Optional[str] = None) -> ImageFont.FreeTypeFont:
    """Get the best available font for the given family and size."""
    font_paths = []
    
    if font_family:
        # Try user-specified font family first
        font_paths.extend([
            f"{font_family}.ttf",
            f"/usr/share/fonts/truetype/{font_family.lower()}/{font_family}.ttf",
            f"/System/Library/Fonts/{font_family}.ttc",
        ])
    
    # Fallback fonts
    font_paths.extend([
        "arial.ttf",
        "Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf", 
        "/System/Library/Fonts/Arial.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ])
    
    # Try each font path
    for font_path in font_paths:
        try:
            font = ImageFont.truetype(font_path, font_size)
            logger.info(f"Using font: {font_path}")
            return font
        except (OSError, IOError):
            continue
    
    # Final fallback to default font
    logger.warning("Could not load any TrueType font, using default")
    return ImageFont.load_default()


def calculate_text_size(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.FreeTypeFont) -> tuple[int, int]:
    """Calculate text bounding box size."""
    bbox = draw.textbbox((0, 0), text, font=font)
    return bbox[2] - bbox[0], bbox[3] - bbox[1]


def create_branding_image(
    text: str,
    output_path: Path,
    text_color: tuple[int, int, int, int],
    width: int = 400,
    height: int = 120,
    font_size: int = 48,
    font_family: Optional[str] = None,
    background_color: Optional[tuple[int, int, int, int]] = None
) -> None:
    """Create a branding image with the specified text and styling."""
    
    logger.debug(f"Creating branding image: {text} -> {output_path}")
    
    # Create image with transparent or colored background
    bg_color = background_color or (0, 0, 0, 0)  # Transparent by default
    img = Image.new('RGBA', (width, height), bg_color)
    draw = ImageDraw.Draw(img)
    
    # Get font
    font = get_font(font_size, font_family)
    
    # Calculate text position to center it
    text_width, text_height = calculate_text_size(draw, text, font)
    x = (width - text_width) // 2
    y = (height - text_height) // 2
    
    # Draw text
    draw.text((x, y), text, fill=text_color, font=font)
    
    # Save image
    try:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        img.save(output_path, 'PNG')
        logger.info(f"Created branding image: {output_path}")
    except Exception as e:
        logger.error(f"Failed to save {output_path}: {e}")
        raise


def main():
    parser = argparse.ArgumentParser(
        description='Generate branding images with text for light and dark themes'
    )
    parser.add_argument(
        '-p', '--pubspec',
        type=Path,
        default=Path('pubspec.yaml'),
        help='Path to pubspec.yaml (default: pubspec.yaml)'
    )
    parser.add_argument(
        '-t', '--text',
        type=str,
        help='Text to render (overrides pubspec config, default: WebFly)'
    )
    parser.add_argument(
        '-o', '--output-dir',
        type=Path,
        help='Output directory for generated images (default: assets/gen)'
    )
    parser.add_argument(
        '--width',
        type=int,
        help='Image width in pixels (default: from pubspec or 400)'
    )
    parser.add_argument(
        '--height',
        type=int,
        help='Image height in pixels (default: from pubspec or 120)'
    )
    parser.add_argument(
        '--font-size',
        type=int,
        help='Font size in pixels (default: from pubspec or 48)'
    )
    parser.add_argument(
        '--font-family',
        type=str,
        help='Font family name (default: from pubspec or system default)'
    )
    parser.add_argument(
        '--light-color',
        type=str,
        help='Text color for light theme as hex (e.g., #323232)'
    )
    parser.add_argument(
        '--dark-color',
        type=str,
        help='Text color for dark theme as hex (e.g., #DCDCDC)'
    )
    parser.add_argument(
        '--background',
        action='store_true',
        help='Add solid background instead of transparent'
    )
    parser.add_argument(
        '-v', '--verbose',
        action='store_true',
        help='Enable verbose logging'
    )

    args = parser.parse_args()

    if args.verbose:
        logger.setLevel(logging.DEBUG)

    # Read pubspec.yaml for configuration
    pubspec = load_pubspec(args.pubspec)
    branding_config = pubspec.get('gen-branding', {})
    
    # Get configuration values (args override pubspec)
    text = args.text or branding_config.get('text', 'WebFly')
    output_dir = args.output_dir or Path(branding_config.get('output_dir', 'assets/gen'))
    width = args.width or branding_config.get('width', 400)
    height = args.height or branding_config.get('height', 120)
    font_size = args.font_size or branding_config.get('font_size', 48)
    font_family = args.font_family or branding_config.get('font_family')
    output_pattern = branding_config.get('output_pattern', '{text}_branding_{theme}.png')
    
    # Parse colors
    def hex_to_rgba(hex_color: str) -> tuple[int, int, int, int]:
        """Convert hex color string to RGBA tuple."""
        hex_color = hex_color.lstrip('#')
        if len(hex_color) == 6:
            return tuple(int(hex_color[i:i+2], 16) for i in (0, 2, 4)) + (255,)
        elif len(hex_color) == 8:
            return tuple(int(hex_color[i:i+2], 16) for i in (0, 2, 4, 6))
        else:
            raise ValueError(f"Invalid hex color: {hex_color}")
    
    # Default colors
    default_light_color = (50, 50, 50, 255)      # Dark gray for light theme
    default_dark_color = (220, 220, 220, 255)    # Light gray for dark theme
    
    light_color = default_light_color
    dark_color = default_dark_color
    
    # Override with config or args
    if args.light_color:
        light_color = hex_to_rgba(args.light_color)
    elif 'light_color' in branding_config:
        light_color = hex_to_rgba(branding_config['light_color'])
        
    if args.dark_color:
        dark_color = hex_to_rgba(args.dark_color)
    elif 'dark_color' in branding_config:
        dark_color = hex_to_rgba(branding_config['dark_color'])
    
    # Background colors (if enabled)
    light_bg = (255, 255, 255, 255) if args.background else None  # White
    dark_bg = (18, 18, 18, 255) if args.background else None      # Dark gray
    
    logger.info(f"Generating branding images for: '{text}'")
    logger.info(f"Output directory: {output_dir}")
    logger.info(f"Dimensions: {width}x{height}")
    logger.info(f"Font size: {font_size}")
    if font_family:
        logger.info(f"Font family: {font_family}")

    try:
        # Normalize text for filename
        text_normalized = text.lower().replace(' ', '_')
        
        # Generate light theme image
        light_filename = output_pattern.format(text=text_normalized, theme='light')
        light_output = output_dir / light_filename
        create_branding_image(
            text=text,
            output_path=light_output,
            text_color=light_color,
            width=width,
            height=height,
            font_size=font_size,
            font_family=font_family,
            background_color=light_bg
        )
        
        # Generate dark theme image
        dark_filename = output_pattern.format(text=text_normalized, theme='dark')
        dark_output = output_dir / dark_filename
        create_branding_image(
            text=text,
            output_path=dark_output,
            text_color=dark_color,
            width=width,
            height=height,
            font_size=font_size,
            font_family=font_family,
            background_color=dark_bg
        )
        
        logger.info(f"Successfully generated branding images in {output_dir}/")
        
    except Exception as e:
        logger.error(f"Failed to generate branding images: {e}")
        sys.exit(1)


if __name__ == '__main__':
    main()
