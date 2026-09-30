#!/usr/bin/env bash
# Epicode 安全发布脚本（防"老进程抢端口 → systemd 崩溃循环"）
# 流程：体检 → 停 systemd(service+socket) → 强制杀孤儿 → 等端口释放 → 原子替换 → 启 → 等就绪 → 验证
# 用法：sudo /opt/tetramem/deploy_safe.sh /path/to/new/binary
#
# 2026-10-01 修复: epicode.socket(systemd socket activation)会在 stop service 后
# 自动拉起新进程占住 9111 → "等端口释放"超时 FATAL(2026-09-30 RBAC部署实测)。
# 现在 [2/7] 同时 stop socket+service。
# 本文件与服务器 /opt/tetramem/deploy_safe.sh 保持同步，纳入版本控制。

set -euo pipefail

SVC="${SVC:-epicode.service}"
SOCKET_SVC="${SOCKET_SVC:-epicode.socket}"
BIN_DST="/opt/tetramem/epicode-cloud"
PORT="${PORT:-9111}"
# ONNX 模型加载 + 记忆恢复需 2-4 分钟,240s 太短会误触发回滚。默认 360s。
TIMEOUT_WAIT="${TIMEOUT_WAIT:-360}"

[[ $# -ge 1 ]] || { echo "usage: $0 <new-binary>"; exit 2; }
NEW="$1"
[[ -f "$NEW" ]] || { echo "ERR: $NEW not found"; exit 2; }

ts(){ date +%Y%m%d_%H%M%S; }
log(){ echo "[$(ts)] $*"; }

log "=== [1/7] 体检 ==="
PIDS_OLD="$(pgrep -af "epicode-cloud" 2>/dev/null | head -20 || true)"
if [[ -n "$PIDS_OLD" ]]; then log "现有进程：$PIDS_OLD"; else log "无现存进程"; fi
if ss -lntH "( sport = :$PORT )" 2>/dev/null | grep -q LISTEN; then log "端口 $PORT 当前被占用"; else log "端口 $PORT 空闲"; fi

log "=== [2/7] 停 systemd(service+socket) ==="
# socket activation 修复: 只停 service 时 socket 单元会自动拉起新实例占端口
if systemctl is-active --quiet "$SOCKET_SVC"; then systemctl stop "$SOCKET_SVC"; log "systemctl stop $SOCKET_SVC 已发出"; fi
if systemctl is-active --quiet "$SVC"; then systemctl stop "$SVC"; log "systemctl stop $SVC 已发出"; else log "systemd 已不活跃"; fi

log "=== [3/7] 杀孤儿（兜底）==="
for pid in $(pgrep -x epicode-cloud 2>/dev/null || true); do
  log "kill -TERM $pid"; kill -TERM "$pid" 2>/dev/null || true
done
sleep 5
for pid in $(pgrep -x epicode-cloud 2>/dev/null || true); do
  log "kill -KILL $pid (TERM未退)"; kill -KILL "$pid" 2>/dev/null || true
done
sleep 1

log "=== [4/7] 等端口释放（最多 ${TIMEOUT_WAIT}s）==="
WAITED=0
while ss -lntH "( sport = :$PORT )" 2>/dev/null | grep -q LISTEN; do
  if [[ $WAITED -ge $TIMEOUT_WAIT ]]; then
    echo "FATAL: 端口 $PORT 未释放！"
    ss -lntp "( sport = :$PORT )" || true
    exit 1
  fi
  sleep 5; WAITED=$((WAITED+5))
done
log "端口已释放"

log "=== [5/7] 原子替换 ==="
BACKUP="${BIN_DST}.bak.$(ts)"
cp "$BIN_DST" "$BACKUP" && log "备份 → $BACKUP"
cp "$NEW" "$BIN_DST" && chmod +x "$BIN_DST" && log "替换完成"

log "=== [6/7] 启动(socket+service) ==="
systemctl start "$SOCKET_SVC" "$SVC" && log "启动命令已发出"

log "=== [7/7] 等就绪 ==="
WAITED=0
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do
  if [[ $WAITED -ge $TIMEOUT_WAIT ]]; then
    log "就绪超时 — 回滚"
    cp "$BACKUP" "$BIN_DST"
    systemctl restart "$SOCKET_SVC" "$SVC"
    exit 1
  fi
  sleep 5; WAITED=$((WAITED+5))
done
log "服务就绪 ✓ 发布完成"
