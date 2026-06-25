# Development Guide

This guide helps contributors set up a local development environment, debug Epicode, and follow project conventions.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.88+ | Backend + guard |
| Node.js | 20+ | Frontend + TS SDK |
| Python | 3.10+ | Python SDK + tests |
| SQLite | 3.34+ | Local database |
| OpenSSL | 1.1+ / 3.x | TLS, crypto |
| ONNX Runtime | auto (bundled via `ort` crate) | Embedding inference |

macOS users: `brew install sqlite openssl pkg-config`.
Linux users: `sudo apt-get install libsqlite3-dev pkg-config libssl-dev`.

## Local Development

### 1. Clone & bootstrap

```bash
git clone https://github.com/sunormesky-max/epicode.git
cd epicode
```

### 2. Backend

```bash
cd backend
cargo run                    # single-tenant mode, listens on :9110
# or
cargo run --bin epicode-cloud # multi-tenant cloud mode, listens on :9111
```

Required environment variables:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `EPICODE_ADMIN_KEY` | yes | — | Admin token for management APIs |
| `EPICODE_API_KEY` | yes (prod) | — | API key clients must send via `X-API-Key` |
| `EPICODE_MASTER_KEY` | yes (prod) | — | 32-byte base64 key for at-rest encryption |
| `DEEPSEEK_API_KEY` | optional | — | For LLM classification features |
| `REDIS_URL` | optional | — | Redis for cloud mode rate limiting |
| `EPICODE_LISTEN_ADDR` | optional | `0.0.0.0:9111` | Bind address |
| `TETRAMEM_*` | legacy | — | Old env prefix, still accepted as fallback |

Generate a master key:

```bash
openssl rand -base64 32
```

### 3. Frontend

```bash
cd frontend
npm ci
npm run dev   # Vite dev server on :5173, proxies /api → http://localhost:9111
```

### 4. Guard (optional, self-hosted defense)

```bash
cd guard
cargo run --release
```

## Testing

### Backend

```bash
cd backend
cargo test --all-targets          # unit + integration tests
cargo test --test crypto           # crypto module only
cargo test --test util             # util module only
cargo fmt --all --check            # formatting check
cargo clippy --all-targets -- -D warnings
```

### Frontend

```bash
cd frontend
npm run test    # vitest (unit tests)
npm run lint    # eslint
npm run check   # tsc -b (type check)
npm run build   # production build
```

### Python / TS SDKs

```bash
# Python
cd backend/sdk/python
pip install -e .
python -m py_compile epicode/*.py

# TypeScript
cd backend/sdk/typescript
npm ci
npx tsc --noEmit
```

### Version consistency

```bash
bash scripts/verify-version.sh
```

This script verifies that `version.txt`, both `Cargo.toml`, `frontend/package.json`, both SDK manifests, `pyproject.toml`, `__init__.py`, `openapi.yaml`, and `.release-please-manifest.json` all carry the same version number.

## Pre-commit Hooks

We use [pre-commit](https://pre-commit.com/) to run formatting and lint checks before each commit.

```bash
pip install pre-commit
pre-commit install
pre-commit run --all-files
```

The configuration (`.pre-commit-config.yaml`) runs:

- `cargo fmt --check` (backend + guard)
- `cargo clippy` (backend)
- `eslint` (frontend)
- `tsc -b` (frontend)
- `scripts/verify-version.sh`
- Standard file hygiene (trailing whitespace, YAML/TOML/JSON validation, private key detection)

## Debugging

### Backend logging

```bash
RUST_LOG=debug cargo run
RUST_LOG=epicode::engine::crypto=trace cargo run  # per-module trace
```

### Inspecting the SQLite database

```bash
sqlite3 backend/data/tetramem.db
.tables
.schema memories
```

> Note: The database file is named `tetramem.db` for backward compatibility. It will be renamed to `epicode.db` in a future major release.

### Frontend dev tools

- React DevTools browser extension
- Vite dev overlay shows HMR errors inline
- `npm run dev` proxies `/api` to the backend; no CORS config needed locally

## Conventions

### Commits

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add energy cap configuration
fix: prevent duplicate context_observe events
chore: bump dependencies
docs: update deployment guide
```

### API prefix

- Public API: `/api/v1/*` (Nginx strips `/api`, backend serves `/v1/*`)
- Backend direct (dev): `http://localhost:9110/v1/remember`
- Through Nginx (prod): `https://epicode.cn/api/v1/remember`
- Health: `/health` (no auth, no `/v1` prefix in cloud mode)

### Environment variables

- New variables use the `EPICODE_` prefix
- The `TETRAMEM_` prefix is accepted as a fallback for backward compatibility
- Both prefixes are documented in `SECURITY.md` and this guide

## Common Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| `error: linking with cc failed` | Missing OpenSSL dev headers | `brew install openssl` / `apt-get install libssl-dev` |
| `EPICODE_MASTER_KEY not set` | Master key env var missing | `export EPICODE_MASTER_KEY=$(openssl rand -base64 32)` |
| Frontend `fetch failed` in dev | Backend not running | Start backend on :9111 or adjust Vite proxy |
| `cargo audit` reports vulnerabilities | Outdated transitive deps | Run `cargo update` and re-audit |
| ONNX model not found | Missing model files | Run the backend once to auto-download, or place models in `backend/models/` |

## Next Steps

- Read `docs/architecture.md` for the system design
- Read `docs/api-reference.md` for the REST API
- Read `docs/deployment.md` for production deployment
- Read `CONTRIBUTING.md` for the contribution workflow
