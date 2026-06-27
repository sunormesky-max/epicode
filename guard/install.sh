#!/bin/bash
set -euo pipefail

echo "[1] Creating directories..."
mkdir -p /var/lib/epicode-guard
mkdir -p /var/log/epicode-guard
chmod 700 /var/lib/epicode-guard
chmod 700 /var/log/epicode-guard

echo "[2] Installing binary..."
cp epicode-guard /usr/local/bin/epicode-guard
chmod 700 /usr/local/bin/epicode-guard

echo "[3] Verifying nftables backend..."
# epicode-guard v3+ uses nftables directly. The old ipset+firewalld rich rule
# path (managed here in v2) is NOT used at runtime and was left dangling,
# producing an empty `epicode-ban` ipset attached to a permanent firewalld
# rule — making operators believe firewalling was active when it was inert.
# We therefore verify `nft` is available and remove any leftover ipset/rich
# rule from prior installs.
if ! command -v nft >/dev/null 2>&1; then
    echo "ERROR: nftables (nft) is required but not installed. Install nftables and re-run." >&2
    exit 1
fi

# Clean up any legacy v2 ipset + firewalld rich rule from previous installs.
ipset destroy epicode-ban 2>/dev/null || true
LEGACY_RULE=$(firewall-cmd --list-rich-rules 2>/dev/null | grep 'ipset=epicode-ban' || true)
if [ -n "$LEGACY_RULE" ]; then
    firewall-cmd --permanent --remove-rich-rule="$LEGACY_RULE" 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
    echo "  Removed legacy v2 firewalld ipset rich rule"
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
