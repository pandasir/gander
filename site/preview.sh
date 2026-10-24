#!/usr/bin/env bash
# Preview the site locally, at http://127.0.0.1:8899/.
#
# The pages refer to the screenshots by bare filename, because on Pages they sit
# next to index.html at the root of the published tree. Locally they live in
# assets/, so this copies them in first — and the stylesheet is already in site/,
# so every page loads from here with no path rewriting.
#
# The copies are gitignored, so this never becomes a second source of truth.
#
# Usage: site/preview.sh [port]
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
port=${1:-8899}

cp -f "$root"/assets/*.png "$here"/
cp -f "$root"/assets/demo.gif "$here"/
cp -f "$root"/assets/demo.webm "$here"/
cp -f "$root"/assets/demo.mp4 "$here"/

# Fail loudly rather than serving a page whose stylesheet is missing: that looks
# like a CSS bug and is really a packaging one.
for f in index.html site.css; do
  test -s "$here/$f" || { echo "missing $here/$f" >&2; exit 1; }
done

echo "pages: $(cd "$here" && ls -1 *.html | tr '\n' ' ')"
echo "serving $here at http://127.0.0.1:$port/"
exec python3 -m http.server "$port" --directory "$here"
