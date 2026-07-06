#!/usr/bin/env bash
# Transfer a repository to the kotro-labs GitHub organization.
# Usage: ORG_NAME=kotro-labs ./scripts/migrate-github-org.sh
set -euo pipefail

ORG_NAME="${ORG_NAME:-kotro-labs}"
REPO="${REPO:-kotro-labs/kotro-proxy-engine}"
POLL_SECS="${POLL_SECS:-5}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-600}"

echo "Target organization: ${ORG_NAME}"
echo "Repository to transfer: ${REPO}"
echo ""

if [[ "${ORG_NAME}" == "kotro" ]]; then
  if curl -fsS "https://api.github.com/users/kotro" | grep -q '"type": "User"'; then
    echo "ERROR: github.com/kotro is a personal account (since 2015), not an organization."
    echo "       GitHub does not allow an org with the same login. Use ORG_NAME=kortosystems instead."
    exit 1
  fi
fi

echo "Waiting for organization https://github.com/${ORG_NAME} (up to ${MAX_WAIT_SECS}s)..."
echo "If you have not created it yet, open:"
echo "  https://github.com/organizations/plan?organization_name=${ORG_NAME}"
echo ""

elapsed=0
while (( elapsed < MAX_WAIT_SECS )); do
  if gh api "orgs/${ORG_NAME}" --jq .login >/dev/null 2>&1; then
    echo "Organization ${ORG_NAME} found."
    break
  fi
  sleep "${POLL_SECS}"
  elapsed=$((elapsed + POLL_SECS))
  echo "  ... still waiting (${elapsed}s)"
done

if ! gh api "orgs/${ORG_NAME}" --jq .login >/dev/null 2>&1; then
  echo "ERROR: Organization ${ORG_NAME} not found after ${MAX_WAIT_SECS}s."
  exit 1
fi

echo ""
echo "Transferring ${REPO} -> ${ORG_NAME}/kotro-proxy-engine ..."
gh api \
  --method POST \
  -H "Accept: application/vnd.github+json" \
  "/repos/${REPO}/transfer" \
  -f new_owner="${ORG_NAME}"

echo ""
echo "Transfer initiated. Verifying destination repo..."
for _ in $(seq 1 30); do
  if gh repo view "${ORG_NAME}/kotro-proxy-engine" --json nameWithOwner -q .nameWithOwner >/dev/null 2>&1; then
    gh repo view "${ORG_NAME}/kotro-proxy-engine" --json url -q '"Live at: " + .url'
    echo ""
    echo "Update local remote:"
    echo "  git remote set-url origin git@github.com:${ORG_NAME}/kotro-proxy-engine.git"
  exit 0
  fi
  sleep 2
done

echo "Transfer submitted; repo may still be propagating. Check:"
echo "  https://github.com/${ORG_NAME}/kotro-proxy-engine"
