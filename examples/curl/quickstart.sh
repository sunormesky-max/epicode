#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${EPICODE_BASE_URL:-http://localhost:8080/api}"
API_KEY="${EPICODE_API_KEY:-}"

if [ -z "$API_KEY" ]; then
  echo "Set EPICODE_API_KEY before running this script."
  exit 1
fi

echo "== health =="
curl -s "${BASE_URL%/api}/health" | jq . || curl -s "${BASE_URL%/api}/health"

echo
echo "== remember =="
curl -s -X POST "$BASE_URL/v1/remember" \
  -H "Content-Type: application/json" \
  -H "X-API-Key: $API_KEY" \
  -d '{"content":"Epicode quickstart stored from curl example"}'

echo
echo "== search =="
curl -s -X POST "$BASE_URL/v1/search" \
  -H "Content-Type: application/json" \
  -H "X-API-Key: $API_KEY" \
  -d '{"query":"curl example","limit":5}'

echo
echo "== ask =="
curl -s -X POST "$BASE_URL/v1/ask" \
  -H "Content-Type: application/json" \
  -H "X-API-Key: $API_KEY" \
  -d '{"question":"What did the curl example store?","depth":2}'
