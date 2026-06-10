#!/bin/bash
set -euo pipefail
# Epicode E2E test

BIN="${1:-./target/release/epicode}"
DATA_DIR=$(mktemp -d)
INPUT_FILE="./deploy/e2e_test.jsonl"

if [ ! -f "$INPUT_FILE" ]; then
    echo "Creating test input..."
    cat > "$INPUT_FILE" << 'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-test","version":"1.0"}}}
{"jsonrpc":"2.0","id":2,"method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_create","arguments":{"content":"Server deployment test memory","labels":["test","deployment"]}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"deployment test","limit":3}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"space_stats","arguments":{}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"dream_cycle","arguments":{}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"ctx_load","arguments":{}}}
EOF
fi

echo "=== Epicode E2E Test ==="
echo "Binary:  $BIN"
echo "DataDir: $DATA_DIR"
echo ""

export TETRAMEM_DATA_DIR="$DATA_DIR"
START=$(date +%s%N)

OUTPUT=$("$BIN" < "$INPUT_FILE" 2>"$DATA_DIR/stderr.txt")

END=$(date +%s%N)
ELAPSED=$(( (END - START) / 1000000 ))

RESPONSES=$(echo "$OUTPUT" | grep -c '"jsonrpc"')
ERRORS=$(echo "$OUTPUT" | grep -c '"error"')

echo "Time:      ${ELAPSED}ms"
echo "Responses: ${RESPONSES}"
echo "Errors:    ${ERRORS}"
echo ""
echo "Stderr:"
cat "$DATA_DIR/stderr.txt" | grep -E "WARN|ERROR|INFO|VectorLayer"

# Cleanup
rm -rf "$DATA_DIR"

if [ "$ERRORS" -le 2 ] && [ "$RESPONSES" -ge 5 ]; then
    echo ""
    echo "=== E2E PASSED ==="
    exit 0
else
    echo ""
    echo "=== E2E FAILED ==="
    exit 1
fi
