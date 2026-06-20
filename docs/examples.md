# Integration Examples

Epicode now includes runnable examples under `examples/`.

## Available examples

| Path | Runtime | What it shows |
| --- | --- | --- |
| `examples/curl/quickstart.sh` | curl | health, remember, search, ask |
| `examples/node/basic-memory.mjs` | Node.js 18+ | minimal memory workflow with built-in `fetch` |
| `examples/python/basic_memory.py` | Python 3 | minimal memory workflow with stdlib `urllib` |

## Shared assumptions

All examples use the same public API shape:

- base URL defaults to `http://localhost:8080/api`
- authentication uses `X-API-Key`
- requests are standard JSON over HTTP

## Existing official SDKs

The repository already ships source SDKs here:

- `backend/sdk/python`
- `backend/sdk/typescript`

Those SDKs are best when you want a reusable client library instead of a single script.
