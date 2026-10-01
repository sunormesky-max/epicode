# Epicode MCP Bridge

A lightweight stdio [Model Context Protocol](https://modelcontextprotocol.io) (MCP) adapter for the [Epicode](https://epicode.cn) REST API. It intentionally exposes a small memory-focused subset; use the cloud `/mcp` endpoint for the full native tool catalog, including identity, skills, projects, and L0 drive tools.

## Tools

| Tool | Endpoint | Description |
|------|----------|-------------|
| `memory_create(content, labels?)` | `POST /api/v1/remember` | Store a memory and return its SMRP placement result. |
| `memory_search(query, limit?, offset?, filters...)` | `POST /api/v1/search` | Inspect ranked memory evidence, tiers, and provenance. REST default is 20 results. |
| `memory_recall(query, depth?)` | `POST /api/v1/recall` | Retrieve associative context using SMRP tiers. |
| `memory_ask(question, depth?)` | `POST /api/v1/ask` | Return the existing memory-grounded answer and structured source memories. |
| `health()` | `GET /health` | Check Epicode cloud connectivity. |

Object results expose `structuredContent` through FastMCP and retain JSON text content for hosts that only consume text. The memory operations return the REST SMRP envelope without flattening its `protocol`, `data`, or `status` fields. The bridge requires `mcp>=1.26.0` for this structured return behavior.

## Install

1. Create and activate a virtual environment:

   ```bash
   python -m venv .venv
   # macOS/Linux
   source .venv/bin/activate
   # Windows PowerShell
   # .venv\Scripts\Activate.ps1
   ```

2. Install dependencies:

   ```bash
   python -m pip install -r mcp-bridge/requirements.txt
   ```

3. Set the API key and optional base URL in the environment:

   ```bash
   # macOS/Linux
   export EPICODE_API_KEY=tm-...
   export EPICODE_BASE_URL=https://epicode.cn
   # Windows PowerShell
   # $env:EPICODE_API_KEY = "tm-..."
   # $env:EPICODE_BASE_URL = "https://epicode.cn"
   ```

## Run

```bash
python mcp-bridge/epicode_mcp_server.py
```

The server runs in `stdio` mode and is meant to be launched by an MCP host such as Claude Desktop or Cursor.

## Configure Claude Desktop

1. Open Claude Desktop → Settings → Developer → Edit Config.
2. Merge the contents of `mcp-bridge/claude_desktop_config.json.example` into your `claude_desktop_config.json`.
3. Replace the command and script placeholders with the absolute paths to your Python interpreter and this repository's `mcp-bridge/epicode_mcp_server.py`.
4. Set `EPICODE_API_KEY` to your real key and `EPICODE_BASE_URL` to your deployment URL.
5. Restart Claude Desktop.

## Configure Cursor

1. Open Cursor → Settings → MCP.
2. Add a new MCP server and paste the contents of `mcp-bridge/cursor_mcp_config.json.example`.
3. Replace the command and script placeholders with the absolute paths to your Python interpreter and this repository's `mcp-bridge/epicode_mcp_server.py`.
4. Set `EPICODE_API_KEY` to your real key and `EPICODE_BASE_URL` to your deployment URL.
5. Save and reload the window (`Cmd/Ctrl + Shift + P` → "Developer: Reload Window").

## Security note

Never commit your real API key. `.env.example` and the `*.example` config files contain placeholders only. Keep your actual key in environment variables or a local `.env` file that is gitignored.
