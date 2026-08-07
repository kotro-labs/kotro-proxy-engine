#!/usr/bin/env bash
# Option A staging (hardened) — tracked files at a pinned revision → clean staging dir.
# Spike utility toward product; Sol staging blockers addressed.
#
# Usage:
#   ./stage-repo.sh --repo <path> [--rev HEAD] [--preview]
#   ./stage-repo.sh --repo <path> --include-untracked rel/path1 rel/path2
#
# Staging dirs are created ONLY under KOTRO_STAGING_ROOT (default: ~/.kotro/staging).
# Authoritative manifest is written host-side next to the staging dir (*.manifest.jsonl),
# never inside the agent-mounted tree.
set -euo pipefail

REPO=""
REV="HEAD"
PREVIEW=0
EXTRA=()
STAGING_ROOT="${KOTRO_STAGING_ROOT:-${HOME}/.kotro/staging}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPO="$2"; shift 2 ;;
    --rev) REV="$2"; shift 2 ;;
    --preview) PREVIEW=1; shift ;;
    --include-untracked) shift; EXTRA=("$@"); break ;;
    --staging-root) STAGING_ROOT="$2"; shift 2 ;;
    -h|--help)
      sed -n '1,16p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1 (note: --out removed; dirs are allocated under staging root)" >&2; exit 2 ;;
  esac
done

[[ -n "$REPO" ]] || { echo "need --repo" >&2; exit 2; }
REPO="$(cd "$REPO" && pwd -P)"

# Worktree detection (supports .git file worktrees)
if ! git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "not a git work tree: $REPO" >&2
  exit 2
fi

is_denied_name() {
  local name="$1"
  case "$name" in
    .env|.env.*|*.pem|*.key|id_rsa|id_rsa.*|.git|KOTRO_STAGING_MANIFEST.txt) return 0 ;;
  esac
  return 1
}

# Reject path escape: must be relative, no .. components, stay under repo when resolved.
normalize_rel_path() {
  local raw="$1"
  if [[ -z "$raw" || "$raw" == /* ]]; then
    echo "REJECT absolute or empty path: $raw" >&2
    return 1
  fi
  local IFS='/'
  local -a parts=()
  read -r -a parts <<< "$raw"
  local p
  for p in "${parts[@]}"; do
    if [[ "$p" == ".." ]]; then
      echo "REJECT path traversal: $raw" >&2
      return 1
    fi
  done
  local resolved
  resolved="$(python3 -c 'import os,sys
repo, raw = sys.argv[1], sys.argv[2]
if ".." in raw.split(os.sep):
    raise SystemExit(2)
print(os.path.realpath(os.path.join(repo, raw)))
' "$REPO" "$raw")" || {
    echo "REJECT path traversal: $raw" >&2
    return 1
  }
  repo_real="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$REPO")"
  case "$resolved" in
    "$repo_real"|"$repo_real"/*) ;;
    *) echo "REJECT escapes repo: $raw -> $resolved" >&2; return 1 ;;
  esac
  python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$resolved" "$repo_real"
}

PIN="$(git -C "$REPO" rev-parse "$REV")"
REMOTE_ID="$(git -C "$REPO" remote get-url origin 2>/dev/null || echo "local")"
TRACKED_LIST="$(mktemp)"
trap 'rm -f "$TRACKED_LIST"' EXIT

git -C "$REPO" ls-tree -r --name-only "$PIN" > "$TRACKED_LIST"
TRACKED_COUNT="$(wc -l < "$TRACKED_LIST" | tr -d ' ')"

echo "pin=$PIN"
echo "repo=$REPO"
echo "remote_id=$REMOTE_ID"
echo "tracked_count=$TRACKED_COUNT"
echo "staging_root=$STAGING_ROOT"

# Warn: committed paths matching deny patterns still stage
WARN_COMMITTED=0
while IFS= read -r tp; do
  bn="$(basename "$tp")"
  if is_denied_name "$bn"; then
    echo "WARN_COMMITTED_SENSITIVE $tp (tracked @ pin — still staged; deny-list applies to extras only)" >&2
    WARN_COMMITTED=1
  fi
done < "$TRACKED_LIST"
if [[ "$WARN_COMMITTED" -eq 1 ]]; then
  echo "WARN: preview does NOT imply all secrets are excluded from tracked tree" >&2
fi

if [[ "$PREVIEW" -eq 1 ]]; then
  echo "=== PREVIEW tracked @ $PIN (first 50) ==="
  head -50 "$TRACKED_LIST"
  echo "..."
  if [[ ${#EXTRA[@]} -gt 0 ]]; then
    echo "=== PREVIEW requested extras (will validate) ==="
    for p in "${EXTRA[@]}"; do
      if rp="$(normalize_rel_path "$p")"; then
        echo "OK_REL $rp"
      else
        echo "BAD $p"
      fi
    done
  fi
  echo "No files written (--preview)."
  exit 0
fi

mkdir -p "$STAGING_ROOT"
STAGING_ROOT="$(cd "$STAGING_ROOT" && pwd)"
# Allocate NEW directory only under staging root (never rm -rf caller paths)
OUT="$(mktemp -d "${STAGING_ROOT}/stage.XXXXXX")"
# Ensure OUT is strictly under staging root
case "$OUT" in
  "$STAGING_ROOT"/*) ;;
  *) echo "FATAL: mktemp escaped staging root" >&2; exit 1 ;;
esac
# Refuse dangerous destinations
for bad in "/" "$HOME" "$REPO"; do
  if [[ "$OUT" == "$bad" || "$OUT" == "$bad"/* && "$bad" == "/" ]]; then
    echo "FATAL: refusing dangerous out $OUT" >&2; exit 1
  fi
done
if [[ "$OUT" == "$REPO" || "$OUT" == "$HOME" || "$OUT" == "/" ]]; then
  echo "FATAL: refusing out=$OUT" >&2; exit 1
fi

MANIFEST="${OUT}.manifest.jsonl"
: > "$MANIFEST"

# Materialize tracked tree without .git
git -C "$REPO" archive "$PIN" | tar -x -C "$OUT"

# Record tracked file hashes (host-side manifest)
while IFS= read -r tp; do
  [[ -z "$tp" ]] && continue
  f="$OUT/$tp"
  if [[ -f "$f" ]]; then
    hash="$(openssl dgst -sha256 "$f" | awk '{print $NF}')"
    printf '{"path":%s,"type":"tracked","sha256":"%s"}\n' "$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$tp")" "$hash" >> "$MANIFEST"
  elif [[ -d "$f" ]]; then
    printf '{"path":%s,"type":"tracked_dir"}\n' "$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$tp")" >> "$MANIFEST"
  fi
done < "$TRACKED_LIST"

copy_extra_file() {
  local rel="$1"
  local src="$REPO/$rel"
  local dst="$OUT/$rel"
  local name hash
  name="$(basename "$rel")"
  if is_denied_name "$name"; then
    echo "SKIP_DENIED $rel" >&2
    return 0
  fi
  mkdir -p "$(dirname "$dst")"
  cp -a "$src" "$dst"
  hash="$(openssl dgst -sha256 "$dst" | awk '{print $NF}')"
  printf '{"path":%s,"type":"extra","sha256":"%s"}\n' "$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$rel")" "$hash" >> "$MANIFEST"
}

# Copy extras with nested deny enforcement; invalid paths fail closed
EXTRA_ERRORS=0
for p in "${EXTRA[@]+"${EXTRA[@]}"}"; do
  [[ -z "$p" ]] && continue
  if ! rel="$(normalize_rel_path "$p")"; then
    EXTRA_ERRORS=$((EXTRA_ERRORS + 1))
    continue
  fi
  src="$REPO/$rel"
  if [[ ! -e "$src" ]]; then
    echo "SKIP_MISSING $rel" >&2
    EXTRA_ERRORS=$((EXTRA_ERRORS + 1))
    continue
  fi
  if [[ -d "$src" ]]; then
    # Walk files; skip denied names at any level
    while IFS= read -r -d '' f; do
      child_rel="${f#"$REPO"/}"
      bn="$(basename "$f")"
      if [[ "$child_rel" == .git || "$child_rel" == .git/* || "$child_rel" == */.git || "$child_rel" == */.git/* ]]; then
        echo "SKIP_DENIED $child_rel" >&2
        continue
      fi
      if is_denied_name "$bn"; then
        echo "SKIP_DENIED $child_rel" >&2
        continue
      fi
      copy_extra_file "$child_rel"
    done < <(find "$src" -type f -print0)
  elif [[ -f "$src" ]]; then
    copy_extra_file "$rel"
  else
    echo "SKIP_NOT_FILE $rel" >&2
    EXTRA_ERRORS=$((EXTRA_ERRORS + 1))
  fi
done

if [[ "$EXTRA_ERRORS" -gt 0 ]]; then
  echo "FATAL: $EXTRA_ERRORS invalid/missing include-untracked path(s)" >&2
  rm -rf "$OUT"
  rm -f "$MANIFEST"
  exit 1
fi

if [[ -e "$OUT/.git" ]]; then
  echo "FATAL: .git present in staging" >&2
  rm -rf "$OUT"
  exit 1
fi
# Never place host manifest inside agent tree
if [[ -e "$OUT/KOTRO_STAGING_MANIFEST.txt" ]]; then
  echo "FATAL: refusing agent-visible manifest name collision" >&2
  rm -rf "$OUT"
  exit 1
fi

# Header line for operators (host-side only)
{
  echo "{\"pin\":\"$PIN\",\"remote_id\":$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$REMOTE_ID"),\"staging\":\"$OUT\"}"
  cat "$MANIFEST"
} > "${MANIFEST}.tmp"
mv "${MANIFEST}.tmp" "$MANIFEST"

echo "staged=$OUT"
echo "manifest_host=$MANIFEST"
echo "OK"
