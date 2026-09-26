#!/bin/bash
# Epicode MCP over TCP socket (JSON-RPC, one request per line)
# 安全(审计三轮高优): 该通道无认证无 TLS — 默认仅绑定回环;
# 远程使用必须经由带认证的 SSH 隧道/安全代理, 切勿直接暴露到不可信网络
BIND=${EPICODE_BIND:-127.0.0.1}
PORT=${EPICODE_PORT:-19100}
BIN=${EPICODE_BIN:-./target/release/epicode-mcp}

exec socat TCP-LISTEN:${PORT},bind=${BIND},fork,reuseaddr EXEC:${BIN}
