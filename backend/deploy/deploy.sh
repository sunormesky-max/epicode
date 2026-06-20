#!/bin/bash
set -euo pipefail

# Epicode Deployment Script
# Target: Linux x86_64
# Author: sunormesky-max

set -a
DEPLOY_DIR="${EPICODE_DEPLOY_DIR:-/opt/epicode}"
DATA_DIR="${EPICODE_DATA_DIR:-/var/lib/epicode}"
LOG_DIR="${EPICODE_LOG_DIR:-/var/log/epicode}"
BIN_NAME="epicode-cloud"
REPO_URL="https://github.com/sunormesky-max/epicode.git"
MODEL_DIR="${DEPLOY_DIR}/models"
MODEL_URL="https://huggingface.co/sentence-transformers/all-mpnet-base-v2/resolve/main/model.onnx"
TOKENIZER_URL="https://huggingface.co/sentence-transformers/all-mpnet-base-v2/resolve/main/tokenizer.json"
set +a

echo "=== Epicode Deployment ==="
echo "Deploy dir: ${DEPLOY_DIR}"
echo "Data dir:   ${DATA_DIR}"
echo ""

# ---- Step 1: System dependencies ----
echo "[1/8] Installing system dependencies..."
yum install -y gcc gcc-c++ make openssl-devel pkg-config git curl 2>/dev/null || \
dnf install -y gcc gcc-c++ make openssl-devel pkg-config git curl 2>/dev/null || true

# ---- Step 2: Install Rust ----
if ! command -v cargo &>/dev/null; then
    echo "[2/8] Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.88.0
    source "$HOME/.cargo/env"
    echo "Rust $(rustc --version) installed."
else
    echo "[2/8] Rust already installed: $(rustc --version)"
    source "$HOME/.cargo/env" 2>/dev/null || true
fi

# ---- Step 3: Clone source ----
echo "[3/8] Cloning source..."
# NOTE: For production, prefer downloading a pre-built binary instead of cloning source.
if [ -d "${DEPLOY_DIR}/src" ]; then
    cd "${DEPLOY_DIR}"
    git pull origin main || true
else
    git clone "${REPO_URL}" "${DEPLOY_DIR}"
    cd "${DEPLOY_DIR}"
fi

# ---- Step 4: Remove MSVC flags for Linux ----
echo "[4/8] Fixing Cargo config for Linux..."
if [ -f .cargo/config.toml ]; then
    sed -i '/\[env\]/,/^$/d' .cargo/config.toml 2>/dev/null || true
    sed -i '/CFLAGS/d' .cargo/config.toml 2>/dev/null || true
    sed -i '/CXXFLAGS/d' .cargo/config.toml 2>/dev/null || true
fi

# ---- Step 5: Build release ----
echo "[5/8] Building release binary (this takes ~10 minutes on 4 cores)..."
CARGO_BUILD_JOBS=4 cargo build --release --bin epicode-cloud 2>&1 | tail -5
echo "Build complete: $(ls -lh target/release/${BIN_NAME} | awk '{print $5}')"

# ---- Step 6: Download ONNX model ----
echo "[6/8] Downloading all-mpnet-base-v2 ONNX model (415MB)..."
mkdir -p "${MODEL_DIR}"
if [ ! -f "${MODEL_DIR}/model.onnx" ]; then
    curl -L -o "${MODEL_DIR}/model.onnx" "${MODEL_URL}" --progress-bar
    echo "Model downloaded: $(ls -lh ${MODEL_DIR}/model.onnx | awk '{print $5}')"
else
    echo "Model already exists."
fi
if [ ! -f "${MODEL_DIR}/tokenizer.json" ]; then
    curl -L -o "${MODEL_DIR}/tokenizer.json" "${TOKENIZER_URL}" --progress-bar
fi

# ---- Step 7: Create data directory ----
echo "[7/8] Setting up directories..."
mkdir -p "${DATA_DIR}"
mkdir -p "${LOG_DIR}"
useradd -r -s /sbin/nologin epicode 2>/dev/null || true
chown -R epicode:epicode "${DATA_DIR}"
chown -R epicode:epicode "${LOG_DIR}"

# ---- Step 8: Install systemd service ----
echo "[8/8] Installing systemd service..."
cat > /etc/systemd/system/epicode.service << SERVICE
[Unit]
Description=Epicode Memory Server
After=network.target

[Service]
Type=simple
User=epicode
Group=epicode
WorkingDirectory=${DEPLOY_DIR}
ExecStart=${DEPLOY_DIR}/target/release/epicode-cloud
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

Environment=TETRAMEM_DATA_DIR=${DATA_DIR}
Environment=TETRAMEM_MASTER_KEY=__MASTER_KEY_PLACEHOLDER__
Environment=TETRAMEM_API_KEY=__API_KEY_PLACEHOLDER__
Environment=DEEPSEEK_API_KEY=__DEEPSEEK_KEY_PLACEHOLDER__
Environment=RUST_LOG=epicode=info

StandardOutput=journal
StandardError=journal
SyslogIdentifier=epicode

[Install]
WantedBy=multi-user.target
SERVICE

systemctl daemon-reload
echo ""
echo "=== Deployment Complete ==="
echo ""
echo "REQUIRED: Edit /etc/systemd/system/epicode.service and set:"
echo "  TETRAMEM_MASTER_KEY — encryption key (base64, 32 bytes)"
echo "  TETRAMEM_API_KEY    — API authentication key"
echo "  DEEPSEEK_API_KEY    — DeepSeek LLM API key"
echo ""
echo "Then run:"
echo "  systemctl enable epicode"
echo "  systemctl start epicode"
echo "  journalctl -u epicode -f"
