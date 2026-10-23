#!/usr/bin/env bash
# Render assets/social-preview.png from packaging/social-preview.html.
#
# GitHub wants 1280x640 (its recommended size) at under 1 MB, PNG/JPG/GIF. The
# image is committed, so this only runs when the card is edited by hand — it
# needs Chrome, which CI does not have, so it is deliberately not wired in.
#
# Usage: packaging/social-preview.sh
set -euo pipefail

cd "$(dirname "$0")/.."

CHROME=${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}
[ -x "$CHROME" ] || { echo "Chrome not found; set CHROME=/path/to/chrome" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The page loads assets/screenshot-panels.png, so it has to be rendered from the
# repository root rather than from a copy elsewhere.
cp -r assets "$tmp/assets"
cp packaging/social-preview.html "$tmp/card.html"

"$CHROME" --headless=new --disable-gpu --hide-scrollbars \
  --window-size=1280,640 --force-device-scale-factor=2 \
  --user-data-dir="$tmp/profile" \
  --screenshot="$tmp/out.png" "file://$tmp/card.html" 2>/dev/null

# Chrome writes the 2x render; downscale so the text is antialiased at the size
# GitHub actually serves.
magick "$tmp/out.png" -resize 1280x640 -strip assets/social-preview.png

size=$(wc -c < assets/social-preview.png)
[ "$size" -lt 1000000 ] || { echo "social-preview.png is $size bytes; GitHub's limit is 1 MB" >&2; exit 1; }
echo "wrote assets/social-preview.png (${size} bytes)"
