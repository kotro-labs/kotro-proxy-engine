#!/usr/bin/env bash
# R0.3 precursor — staging safety tests (Sol staging blockers)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
STAGE="$ROOT/stage-repo.sh"
chmod +x "$STAGE"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Tiny git repo fixture
REPO="$TMP/repo"
mkdir -p "$REPO/sub"
cd "$REPO"
git init -q
git config user.email "t@t"
git config user.name "t"
echo ok > README.md
echo secret > .env
echo nested > sub/.env
echo safe > notes.txt
git add README.md notes.txt
git commit -q -m init
# leave .env and sub/.env untracked

export KOTRO_STAGING_ROOT="$TMP/kotro-staging"
mkdir -p "$KOTRO_STAGING_ROOT"

echo "--- preview warns committed? (none committed sensitive) ---"
"$STAGE" --repo "$REPO" --preview >/dev/null

echo "--- reject ../ extra ---"
if "$STAGE" --repo "$REPO" --include-untracked '../outside' 2>/dev/null; then
  echo "FAIL: accepted ../"; exit 1
else
  echo "PASS: rejected ../"
fi

echo "--- reject absolute extra ---"
if "$STAGE" --repo "$REPO" --include-untracked '/etc/passwd' 2>/dev/null; then
  echo "FAIL: accepted absolute"; exit 1
else
  echo "PASS: rejected absolute"
fi

echo "--- stage tracked only ---"
OUT_LINE="$("$STAGE" --repo "$REPO" | tee /dev/stderr)"
OUT="$(echo "$OUT_LINE" | sed -n 's/^staged=//p')"
MAN="$(echo "$OUT_LINE" | sed -n 's/^manifest_host=//p')"
test -d "$OUT"
test ! -e "$OUT/.git"
test ! -e "$OUT/KOTRO_STAGING_MANIFEST.txt"
test -f "$MAN"
test -f "$OUT/README.md"
test ! -e "$OUT/.env"
echo "PASS: tracked stage, host manifest, no .git"

echo "--- nested deny in extra dir ---"
mkdir -p "$REPO/wip"
echo x > "$REPO/wip/ok.md"
echo y > "$REPO/wip/.env"
OUT_LINE="$("$STAGE" --repo "$REPO" --include-untracked wip)"
OUT="$(echo "$OUT_LINE" | sed -n 's/^staged=//p')"
test -f "$OUT/wip/ok.md"
test ! -e "$OUT/wip/.env"
echo "PASS: nested .env skipped"

echo "--- manifest has sha256 ---"
grep -q sha256 "$MAN"
echo "PASS: hashes present"

echo "ALL STAGING SAFETY TESTS PASSED"
