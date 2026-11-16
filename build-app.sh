#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_NAME="HP DeskJet 1115.app"
APP_DIR="$ROOT_DIR/$APP_NAME"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

echo "Compilando binarios release..."
cargo build --offline --release --manifest-path "$ROOT_DIR/Cargo.toml"

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$ROOT_DIR/target/release/hp-deskjet-1115" "$MACOS_DIR/hp-deskjet-1115"
cp "$ROOT_DIR/target/release/rastertohp1115" "$MACOS_DIR/rastertohp1115"
chmod +x "$MACOS_DIR/hp-deskjet-1115" "$MACOS_DIR/rastertohp1115"

cat > "$CONTENTS_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>es</string>
    <key>CFBundleExecutable</key>
    <string>hp-deskjet-1115</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.luisfelipe18.hp-deskjet-1115</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>HP DeskJet 1115</string>
    <key>CFBundleDisplayName</key>
    <string>HP DeskJet 1115</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

ICONSET_DIR="$(mktemp -d /tmp/hp1115_icon.XXXXXX)/AppIcon.iconset"
mkdir -p "$ICONSET_DIR"
BASE_PNG="$(dirname "$ICONSET_DIR")/icon_1024.png"

python3 - "$BASE_PNG" <<'PY'
import sys, struct, zlib, math

out_path = sys.argv[1]
size = 512

def png_chunk(tag, data):
    return struct.pack("!I", len(data)) + tag + data + struct.pack("!I", zlib.crc32(tag + data) & 0xffffffff)

def in_round_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = max(x0 + r, min(x, x1 - r))
    cy = max(y0 + r, min(y, y1 - r))
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r

rows = []
for y in range(size):
    row = bytearray([0])
    for x in range(size):
        # Transparent outside macOS-style squircle padding
        if not in_round_rect(x, y, 28, 28, size - 28, size - 28, 96):
            row.extend((0, 0, 0, 0))
            continue

        # Deep HP-inspired blue gradient background
        t = y / (size - 1)
        r = int(12 + 8 * (1 - t))
        g = int(74 + 50 * (1 - t))
        b = int(135 + 65 * (1 - t))
        a = 255

        # Top paper tray sheet
        if in_round_rect(x, y, 156, 86, 356, 190, 12):
            r, g, b = 235, 242, 250
        # Printer main body
        if in_round_rect(x, y, 92, 168, 420, 326, 28):
            r, g, b = 248, 250, 252
        # Printer inner front slot
        if in_round_rect(x, y, 132, 248, 380, 326, 10):
            r, g, b = 30, 41, 59
        # Output paper sheet with CMYK dots
        if in_round_rect(x, y, 156, 286, 356, 422, 10):
            r, g, b = 255, 255, 255
            dots = [
                (198, 354, (0, 174, 239)),
                (236, 354, (236, 0, 140)),
                (274, 354, (250, 204, 21)),
                (312, 354, (24, 24, 28)),
            ]
            for dx, dy, col in dots:
                if (x - dx) ** 2 + (y - dy) ** 2 <= 14 ** 2:
                    r, g, b = col
        # Power/status LED on printer body
        if (x - 376) ** 2 + (y - 208) ** 2 <= 10 ** 2:
            r, g, b = 34, 197, 94

        row.extend((r, g, b, a))
    rows.append(bytes(row))

raw = b"".join(rows)
ihdr = struct.pack("!IIBBBBB", size, size, 8, 6, 0, 0, 0)
png = b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", ihdr) + png_chunk(b"IDAT", zlib.compress(raw, 6)) + png_chunk(b"IEND", b"")
with open(out_path, "wb") as f:
    f.write(png)
PY

for px in 16 32 128 256 512; do
    sips -z "$px" "$px" "$BASE_PNG" --out "$ICONSET_DIR/icon_${px}x${px}.png" >/dev/null
    px2=$((px * 2))
    if [ "$px2" -le 512 ]; then
        sips -z "$px2" "$px2" "$BASE_PNG" --out "$ICONSET_DIR/icon_${px}x${px}@2x.png" >/dev/null
    fi
done
iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/AppIcon.icns"
rm -rf "$(dirname "$ICONSET_DIR")"

codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true

echo "Bundle generado correctamente en: $APP_DIR"
if [ "${1:-}" = "--install" ]; then
    rm -rf "/Applications/$APP_NAME"
    cp -R "$APP_DIR" "/Applications/$APP_NAME"
    echo "Instalado también en: /Applications/$APP_NAME"
fi
