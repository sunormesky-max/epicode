#!/usr/bin/env bash
set -euo pipefail

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

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "All version checks passed!"
    exit 0
else
    echo "Some version checks failed!"
    exit 1
fi
