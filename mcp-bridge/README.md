# Epicode MCP Bridge

A lightweight [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server that exposes the [Epicode](https://epicode.cn) spatial AI memory cloud service as MCP tools.

## Tools

| Tool           | Endpoint                | Description                                  |
|----------------|-------------------------|----------------------------------------------|
| `memory_create`| `POST /api/v1/remember` | Store a new memory                           |
| `memory_search`| `POST /api/v1/search`   | Semantic search over memories                |
| `memory_recall`| `POST /api/v1/recall`   | Associative recall via SMRP                  |
| `memory_ask`   | `POST /api/v1/ask`      | Ask a question grounded in stored memories   |
| `health`       | `GET /health`           | Check Epicode cloud connectivity             |

## Install

1. Create the virtual environment:

   ```bash
   /Users/sunorme/.workbuddy/binaries/python/versions/3.13.12/bin/python3 -m venv /Users/sunorme/.workbuddy/binaries/python/envs/epicode-mcp
   ```

2. Activate it and install dependencies:

   ```bash
   source /Users/sunorme/.workbuddy/binaries/python/envs/epicode-mcp/bin/activate
   pip install -r mcp-bridge/requirements.txt
   ```

3. Copy the example environment file and set your key:

   ```bash
   cp mcp-bridge/.env.example mcp-bridge/.env
   # Edit mcp-bridge/.env and add EPICODE_API_KEY=tm-...
   ```

## Run

```bash
source /Users/sunorme/.workbuddy/binaries/python/envs/epicode-mcp/bin/activate
python mcp-bridge/epicode_mcp_server.py
```

The server runs in `stdio` mode and is meant to be launched by an MCP host such as Claude Desktop or Cursor.

## Configure Claude Desktop

1. Open Claude Desktop → Settings → Developer → Edit Config.
2. Merge the contents of `mcp-bridge/claude_desktop_config.json.example` into your `claude_desktop_config.json`.
3. Replace `your_api_key_here` with your real Epicode API key.
4. The example already points to the venv interpreter (`/Users/sunorme/.workbuddy/binaries/python/envs/epicode-mcp/bin/python3`), so no path changes are needed if you created the venv above.
5. Restart Claude Desktop.

## Configure Cursor

1. Open Cursor → Settings → MCP.
2. Add a new MCP server and paste the contents of `mcp-bridge/cursor_mcp_config.json.example`.
3. Replace `your_api_key_here` with your real Epicode API key.
4. The example already points to the venv interpreter (`/Users/sunorme/.workbuddy/binaries/python/envs/epicode-mcp/bin/python3`), so no path changes are needed if you created the venv above.
5. Save and reload the window (`Cmd/Ctrl + Shift + P` → "Developer: Reload Window").

## Security note

Never commit your real API key. `.env.example` and the `*.example` config files contain placeholders only. Keep your actual key in environment variables or a local `.env` file that is gitignored.
