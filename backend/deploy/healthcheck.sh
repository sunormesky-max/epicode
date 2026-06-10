#!/bin/bash
set -euo pipefail

DATA_DIR="${1:-data}"
BIN="${2:-./target/release/epicode-cloud}"

echo "=== Epicode Health Check ==="

if [ -x "$BIN" ]; then
    echo "[OK] Binary exists: $(ls -lh $BIN | awk '{print $5}')"
else
    echo "[FAIL] Binary not found: $BIN"
    exit 1
fi

if [ -f "./models/model.onnx" ]; then
    SIZE=$(stat -c%s "./models/model.onnx" 2>/dev/null || echo 0)
    echo "[OK] ONNX model: $(( SIZE / 1024 / 1024 ))MB"
else
    echo "[FAIL] ONNX model not found"
fi

if [ -f "./models/tokenizer.json" ]; then
    echo "[OK] Tokenizer exists"
else
    echo "[FAIL] Tokenizer not found"
fi

if [ -d "$DATA_DIR" ]; then
    DB_COUNT=$(ls "$DATA_DIR"/*.db 2>/dev/null | wc -l)
    echo "[OK] Data dir: ${DATA_DIR} (${DB_COUNT} DB files)"
else
    echo "[WARN] Data dir not found: ${DATA_DIR}"
fi

echo ""
echo "=== Done ==="
