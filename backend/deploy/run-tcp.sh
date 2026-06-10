#!/bin/bash
# Epicode MCP over TCP socket
# Accepts JSON-RPC, one request per line
PORT=${TETRAMEM_PORT:-19100}
BIN=${TETRAMEM_BIN:-./target/release/epicode-mcp}

exec socat TCP-LISTEN:${PORT},fork,reuseaddr EXEC:${BIN}
