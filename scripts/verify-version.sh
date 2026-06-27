#!/usr/bin/env bash
# scripts/verify-version.sh — single source of truth for version consistency.
# Pre-commit and CI invoke this from the repo root; guard against accidental
# invocation from a subdirectory by cd'ing to the git toplevel.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

EXPECTED=$(tr -d '[:space:]' < version.txt)
echo "Expected version: $EXPECTED"
FAIL=0

check() {
    local file="$1"
    local pattern="$2"
    local result
    result=$(grep -oE "$pattern" "$file" | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
    if [ "$result" != "$EXPECTED" ]; then
        echo "MISMATCH: $file has '$result', expected '$EXPECTED'"
        FAIL=1
        return
    fi
    echo "OK: $file -> $result"
}

check "version.txt" '[0-9]+\.[0-9]+\.[0-9]+'
check "backend/Cargo.toml" 'version = "[0-9]+\.[0-9]+\.[0-9]+"'
check "guard/Cargo.toml" 'version = "[0-9]+\.[0-9]+\.[0-9]+"'
check "frontend/package.json" '"version": "[0-9]+\.[0-9]+\.[0-9]+"'
check "backend/sdk/python/pyproject.toml" 'version = "[0-9]+\.[0-9]+\.[0-9]+"'
check "backend/sdk/typescript/package.json" '"version": "[0-9]+\.[0-9]+\.[0-9]+"'
check ".release-please-manifest.json" '"[0-9]+\.[0-9]+\.[0-9]+"'

# Helm chart + appVersion. release-please bumps Chart.yaml through extra-files
# (see .release-please-config.json), so these should never drift from version.txt.
check "deploy/helm/epicode/Chart.yaml" '^version: [0-9]+\.[0-9]+\.[0-9]+'
check "deploy/helm/epicode/Chart.yaml" '^appVersion: "[0-9]+\.[0-9]+\.[0-9]+"'

# Check __init__.py
result=$(grep -oE '__version__ = "[0-9]+\.[0-9]+\.[0-9]+"' backend/sdk/python/epicode/__init__.py | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ "$result" != "$EXPECTED" ]; then
    echo "MISMATCH: backend/sdk/python/epicode/__init__.py has '$result', expected '$EXPECTED'"
    FAIL=1
else
    echo "OK: backend/sdk/python/epicode/__init__.py -> $result"
fi

# Check openapi.yaml (indented under info:)
result=$(grep -oE 'version: [0-9]+\.[0-9]+\.[0-9]+' backend/docs/openapi.yaml | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ "$result" != "$EXPECTED" ]; then
    echo "MISMATCH: backend/docs/openapi.yaml info.version has '$result', expected '$EXPECTED'"
    FAIL=1
else
    echo "OK: backend/docs/openapi.yaml -> $result"
fi

# Helm values.yaml image.tag is the single most common source of drift: it is a
# free-form string (not a parseable TOML/JSON version field), so release-please
# cannot update it via `jsonpath`. We still verify it so a release that bumps the
# app version without bumping the image tag fails CI.
result=$(grep -oE 'tag: "[0-9]+\.[0-9]+\.[0-9]+"' deploy/helm/epicode/values.yaml | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ "$result" != "$EXPECTED" ]; then
    echo "MISMATCH: deploy/helm/epicode/values.yaml image.tag has '$result', expected '$EXPECTED'"
    FAIL=1
else
    echo "OK: deploy/helm/epicode/values.yaml image.tag -> $result"
fi

# Static k8s manifest. The manifest pins image tags to the release version
# (e.g. :1.0.1). release-please cannot update it automatically, so this check
# is critical: a release that bumps everything else but leaves the k8s manifest
# pointed at the old tag would cause the cluster to pull the previous release.
result=$(grep -oE 'epicode-(backend|frontend):[0-9]+\.[0-9]+\.[0-9]+' deploy/kubernetes/epicode.yaml | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ "$result" != "$EXPECTED" ]; then
    echo "MISMATCH: deploy/kubernetes/epicode.yaml image tag has '$result', expected '$EXPECTED'"
    FAIL=1
else
    echo "OK: deploy/kubernetes/epicode.yaml image tag -> $result"
fi

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "All version checks passed!"
    exit 0
else
    echo "Some version checks failed!"
    exit 1
fi
