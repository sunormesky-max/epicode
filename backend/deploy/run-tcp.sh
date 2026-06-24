#!/bin/bash
# Epicode MCP over TCP socket
# Accepts JSON-RPC, one request per line
PORT=${EPICODE_PORT:-19100}
BIN=${EPICODE_BIN:-./target/release/epicode-mcp}

exec socat TCP-LISTEN:${PORT},fork,reuseaddr EXEC:${BIN}
