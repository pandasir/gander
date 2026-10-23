#!/usr/bin/env bash
# Assert that the published site's own claims about itself hold.
#
# Three ways this drifts, all of which look like a successful deploy:
#
#   1. A sitemap entry with no page behind it. A crawler is handed a 404, which
#      is worse than not listing the URL at all.
#   2. A page that nothing links to. It is published, indexed, and unreachable
#      by a reader, which is the state the comparison pages were in when they
#      were written: the deploy step copied a hardcoded list of filenames, so a
#      new .html file was silently never published.
#   3. robots.txt disallowing a page that the sitemap advertises. The two files
#      are edited by hand and have no reason to agree.
#
# This runs on the files in site/, with no network and no browser.
#
# Usage: packaging/check-site.sh
set -euo pipefail

cd "$(dirname "$0")/.."

site=site
failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}
ok() { echo "ok: $*"; }

[ -d "$site" ] || { echo "no $site directory" >&2; exit 2; }

mapfile -t pages < <(cd "$site" && ls -1 *.html | sort)
mapfile -t sitemap_urls < <(grep -oE '<loc>[^<]+' "$site/sitemap.xml" | sed 's/<loc>//')

[ "${#pages[@]}" -gt 0 ] || fail "no .html files in $site"

# 1. Every sitemap URL resolves to a file that exists.
for url in "${sitemap_urls[@]}"; do
  case "$url" in
    https://pandasir.github.io/gander/ | https://pandasir.github.io/gander) name=index.html ;;
    https://pandasir.github.io/gander/*) name=${url##*/gander/} ;;
    *) fail "sitemap entry $url is not on the gander Pages origin"; continue ;;
  esac
  [ -f "$site/$name" ] || fail "sitemap lists $url but $site/$name does not exist"
done
ok "every sitemap URL has a page"

# 2. Every page is in the sitemap. A page nobody lists is a page a crawler finds
#    only by luck.
for page in "${pages[@]}"; do
  if [ "$page" = "index.html" ]; then
    found=0
    for url in "${sitemap_urls[@]}"; do
      case "$url" in
        https://pandasir.github.io/gander/ | https://pandasir.github.io/gander) found=1 ;;
      esac
    done
    [ "$found" -eq 1 ] || fail "index.html is not in the sitemap"
  else
    printf '%s\n' "${sitemap_urls[@]}" | grep -q "/$page\$" ||
      fail "$page exists but is not in sitemap.xml"
  fi
done
ok "every page is in the sitemap"

# 3. Every page is reachable from index.html. One hop is enough for a site this
#    size, and it is what a reader does.
for page in "${pages[@]}"; do
  [ "$page" = "index.html" ] && continue
  grep -q "href=\"$page\"" "$site/index.html" ||
    fail "index.html does not link to $page"
done
ok "index.html links every page"

# 4. No page links to a file that is not there. These are the local references
#    only; external URLs are not checked (no network here).
#
#    Screenshots are the exception: the pages refer to them by bare filename
#    because they sit next to index.html once published, and both workflows copy
#    assets/* into the published root. Locally they therefore resolve from
#    assets/, and a reference counts as satisfied if either location has it.
missing_before=$failures
for page in "${pages[@]}"; do
  while IFS= read -r ref; do
    [ -n "$ref" ] || continue
    case "$ref" in
      http*|mailto:*|\#*|//*) continue ;;
    esac
    target=${ref%%#*}
    [ -n "$target" ] || continue
    if [ -e "$site/$target" ] || [ -e "assets/$target" ]; then
      continue
    fi
    fail "$page references missing $target"
  done < <(grep -oE '(href|src)="[^"]+"' "$site/$page" | sed -E 's/^(href|src)="//; s/"$//')
done
if [ "$failures" -eq "$missing_before" ]; then
  ok "no page references a missing local file"
fi

# 5. robots.txt must allow every page the sitemap advertises.
for url in "${sitemap_urls[@]}"; do
  case "$url" in
    https://pandasir.github.io/gander/ | https://pandasir.github.io/gander) name=index.html ;;
    *) name=${url##*/gander/} ;;
  esac
  grep -qx "Allow: /$name" "$site/robots.txt" ||
    fail "robots.txt does not allow /$name, which the sitemap advertises"
done
# The stylesheet is fetched by a crawler rendering the page; disallowing it makes
# every page look unstyled, which is a rendering signal.
grep -qx "Allow: /site.css" "$site/robots.txt" ||
  fail "robots.txt does not allow /site.css, which every page needs"
ok "robots.txt allows every advertised page"

# 6. Each page must declare a canonical URL of its own. A copy-pasted canonical
#    is how a subpage tells a crawler it is a duplicate of the homepage.
for page in "${pages[@]}"; do
  canonical=$(grep -oE 'rel="canonical" href="[^"]+"' "$site/$page" | sed -E 's/.*href="//; s/"$//')
  [ -n "$canonical" ] || { fail "$page has no canonical URL"; continue; }
  case "$page" in
    index.html) want="https://pandasir.github.io/gander/" ;;
    *) want="https://pandasir.github.io/gander/$page" ;;
  esac
  [ "$canonical" = "$want" ] || fail "$page canonical is $canonical, want $want"
done
ok "every page has its own canonical URL"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "site is self-consistent"
