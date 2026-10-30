#!/usr/bin/env bash
# Weekly exposure snapshot for the promotion plan in marketing/README.md.
#
# gh must be authenticated as someone with push access, because the traffic
# endpoints (views/clones/referrers) are owner-only and answer 403 otherwise.
# The site's own numbers live in the GoatCounter dashboard
# (gander.goatcounter.com) and are deliberately not pulled here — that API
# needs a token the repo does not carry, and the dashboard answers the only
# question that matters: which channel sent the visitors.
#
# Usage: .github/scripts/traffic-report.sh [owner/repo]
set -euo pipefail

REPO="${1:-pandasir/gander}"

echo "== GitHub traffic, last 14 days =="
gh api "repos/$REPO/traffic/views" --jq '"  views \(.count) / uniques \(.uniques)"'
gh api "repos/$REPO/traffic/clones" --jq '"  clones \(.count) / uniques \(.uniques)"'

echo
echo "== Referrers =="
gh api "repos/$REPO/traffic/popular/referrers" \
  --jq '.[] | "  \(.referrer): \(.count) views / \(.uniques) uniques"' \
  || echo "  (none recorded)"

echo
echo "== Popular paths =="
gh api "repos/$REPO/traffic/popular/paths" --jq '.[:5][] | "  \(.path): \(.count) views"'

echo
echo "== Repo =="
gh api "repos/$REPO" \
  --jq '"  stars \(.stargazers_count) · forks \(.forks_count) · open issues \(.open_issues_count)"'

echo
echo "== Release asset downloads, all tags =="
gh api "repos/$REPO/releases?per_page=100" \
  --jq '"  \([.[].assets[].download_count] | add // 0)"'

echo
echo "== crates.io (gander-bin) =="
curl -fsSL -H "User-Agent: gander-traffic-report (https://github.com/pandasir/gander)" \
  "https://crates.io/api/v1/crates/gander-bin" \
  | python3 -c 'import json, sys
c = json.load(sys.stdin)["crate"]
print("  total", c["downloads"], "/ last 90d", c["recent_downloads"], "/ version", c["max_version"])'
