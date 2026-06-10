#!/bin/bash
# TetraMem MCP over TCP socket
# Accepts JSON-RPC on port 19100, one request per line
PORT=${TETRAMEM_PORT:-19100}

exec socat TCP-LISTEN:${PORT},fork,reuseaddr EXEC:/opt/tetramem/tetramem-mcp
