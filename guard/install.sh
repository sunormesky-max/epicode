#!/bin/bash
set -euo pipefail

echo "[1] Creating directories..."
mkdir -p /var/lib/epicode-guard
mkdir -p /var/log/epicode-guard

echo "[2] Installing binary..."
cp epicode-guard /usr/local/bin/epicode-guard
chmod 700 /usr/local/bin/epicode-guard

echo "[3] Initializing ipset..."
ipset create epicode-ban hash:ip timeout 0 -exist 2>/dev/null || true

FIREWALL_RULE=$(firewall-cmd --list-rich-rules 2>/dev/null | grep epicode-ban || true)
if [ -z "$FIREWALL_RULE" ]; then
    firewall-cmd --permanent --add-rich-rule='rule source ipset=epicode-ban drop' 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
    echo "  Added firewalld drop rule for epicode-ban ipset"
else
    echo "  Firewalld rule already exists"
fi

echo "[4] Installing systemd service..."
cp epicode-guard.service /etc/systemd/system/epicode-guard.service
systemctl daemon-reload
systemctl enable epicode-guard

echo "[5] Running initial file integrity baseline..."
/usr/local/bin/epicode-guard check

echo ""
echo "=== Install complete ==="
echo "Start: systemctl start epicode-guard"
echo "Status: epicode-guard status"
