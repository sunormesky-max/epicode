#!/bin/bash
set -euo pipefail
# Quick health check for TetraMem

DATA_DIR="${1:-/var/lib/tetramem}"
BIN="/opt/tetramem/target/release/tetramem-mcp"

echo "=== TetraMem Health Check ==="

# Check binary
if [ -x "$BIN" ]; then
    echo "[OK] Binary exists: $(ls -lh $BIN | awk '{print $5}')"
else
    echo "[FAIL] Binary not found: $BIN"
    exit 1
fi

# Check model
if [ -f "/opt/tetramem/models/model.onnx" ]; then
    SIZE=$(stat -f%z "/opt/tetramem/models/model.onnx" 2>/dev/null || stat -c%s "/opt/tetramem/models/model.onnx" 2>/dev/null || echo 0)
    echo "[OK] ONNX model: $(( SIZE / 1024 / 1024 ))MB"
else
    echo "[FAIL] ONNX model not found"
fi

# Check tokenizer
if [ -f "/opt/tetramem/models/tokenizer.json" ]; then
    echo "[OK] Tokenizer exists"
else
    echo "[FAIL] Tokenizer not found"
fi

# Check service
if systemctl is-active --quiet tetramem 2>/dev/null; then
    echo "[OK] Service is running (PID: $(systemctl show tetramem -p MainPID --value))"
else
    echo "[WARN] Service not running"
fi

# Check data directory
if [ -d "$DATA_DIR" ]; then
    DB_COUNT=$(ls "$DATA_DIR"/*.db 2>/dev/null | wc -l)
    echo "[OK] Data dir: ${DATA_DIR} (${DB_COUNT} DB files)"
else
    echo "[WARN] Data dir not found: ${DATA_DIR}"
fi

# Check env vars in service file
MASTER_KEY=$(grep TETRAMEM_MASTER_KEY /etc/systemd/system/tetramem.service 2>/dev/null | cut -d= -f3)
if [ -n "$MASTER_KEY" ] && [ "$MASTER_KEY" != "__MASTER_KEY_PLACEHOLDER__" ]; then
    echo "[OK] TETRAMEM_MASTER_KEY is set"
else
    echo "[FAIL] TETRAMEM_MASTER_KEY not set in service file"
fi

API_KEY=$(grep TETRAMEM_API_KEY /etc/systemd/system/tetramem.service 2>/dev/null | cut -d= -f3)
if [ -n "$API_KEY" ] && [ "$API_KEY" != "__API_KEY_PLACEHOLDER__" ]; then
    echo "[OK] TETRAMEM_API_KEY is set"
else
    echo "[FAIL] TETRAMEM_API_KEY not set in service file"
fi

echo ""
echo "=== Done ==="
