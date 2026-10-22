#!/usr/bin/env bash
# Preview the landing page locally, at http://127.0.0.1:8899/.
#
# index.html refers to the screenshots by bare filename, because on Pages they
# sit next to it at the root of the published tree. Locally they live in
# assets/, so this copies them in first. They are gitignored here, so the copy
# never becomes a second source of truth.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")

cp -f "$root"/assets/*.png "$here"/
cp -f "$root"/assets/demo.gif "$here"/

echo "serving $here at http://127.0.0.1:8899/"
exec python3 -m http.server 8899 --directory "$here"
