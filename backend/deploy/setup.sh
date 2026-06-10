#!/bin/bash
set -euo pipefail

# ============================================================
# TetraMem v14.2.1 — One-command encrypted deployment
# Usage: bash setup.sh <DEEPSEEK_API_KEY>
# ============================================================

DEEPSEEK_KEY="${1:?Usage: bash setup.sh <DEEPSEEK_API_KEY>}"

# --- Generated keys ---
MASTER_KEY="/3NwoHqiE5WWZxyls2r3yL7SwHn3GY6Ek26LgHZwXj4="
API_KEY="Q3EOKNmcC92M0iogdbADZnPVSf5eGvr4"

DEPLOY_DIR="/opt/tetramem"
SERVICE_FILE="/etc/systemd/system/tetramem.service"

echo "=== TetraMem v14.2.1 Encrypted Setup ==="
echo ""

# Step 1: Run deployment
echo "[1/3] Running deployment..."
bash "$(dirname "$0")/deploy.sh"

# Step 2: Inject keys
echo "[2/3] Injecting encryption keys..."
sed -i "s|__MASTER_KEY_PLACEHOLDER__|${MASTER_KEY}|g" "$SERVICE_FILE"
sed -i "s|__API_KEY_PLACEHOLDER__|${API_KEY}|g" "$SERVICE_FILE"
sed -i "s|__DEEPSEEK_KEY_PLACEHOLDER__|${DEEPSEEK_KEY}|g" "$SERVICE_FILE"

# Protect service file
chmod 600 "$SERVICE_FILE"
chown root:root "$SERVICE_FILE"

# Step 3: Start service
echo "[3/3] Starting service..."
systemctl daemon-reload
systemctl enable tetramem
systemctl start tetramem

sleep 3

if systemctl is-active --quiet tetramem; then
    echo ""
    echo "=== TetraMem is running ==="
    echo "PID:     $(systemctl show tetramem -p MainPID --value)"
    echo "Uptime:  $(systemctl show tetramem -p ActiveEnterTimestamp --value)"
    echo ""
    echo "Logs:    journalctl -u tetramem -f"
    echo "Health:  bash $(dirname "$0")/healthcheck.sh"
    echo "Test:    bash $(dirname "$0")/e2e_test.sh"
else
    echo ""
    echo "=== FAILED to start ==="
    journalctl -u tetramem --no-pager -n 30
    exit 1
fi
