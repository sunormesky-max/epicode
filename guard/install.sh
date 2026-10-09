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

# Remove only Epicode's named v2 rule, one rule at a time, from both runtime
# and permanent configuration. A blanket reload would also reset unrelated
# runtime firewall changes made by the operator.
if command -v firewall-cmd >/dev/null 2>&1; then
    for mode in runtime permanent; do
        args=()
        if [ "$mode" = permanent ]; then args=(--permanent); fi
        while IFS= read -r rule; do
            if [[ ( "$rule" == *'source ipset=epicode-ban'* || "$rule" == *'source ipset="epicode-ban"'* ) && "$rule" == *' drop' ]]; then
                firewall-cmd "${args[@]}" --remove-rich-rule="$rule"
                echo "  Removed legacy v2 $mode rule: $rule"
            fi
        done < <(firewall-cmd "${args[@]}" --list-rich-rules 2>/dev/null)
    done
fi
if command -v ipset >/dev/null 2>&1; then
    ipset destroy epicode-ban 2>/dev/null || true
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
