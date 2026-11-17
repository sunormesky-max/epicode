# Configuration

This document describes the environment variables required to configure and deploy Epicode.

## Authentication Keys

These keys control access to the Epicode API and administrative functions.

| Variable | Required | Description |
|----------|----------|-------------|
| `EPICODE_API_KEY` | Yes | Primary API key for authenticating all memory operations (remember, search, recall). Every request must include this key in the `X-API-Key` header. |
| `LLM_API_KEY` | Yes | API key for your LLM provider. Used for cognitive functions including content classification, reasoning, and alias generation. Supports any OpenAI-compatible endpoint via `LLM_API_BASE` and `LLM_MODEL`. |
| `LLM_API_BASE` | No | OpenAI-compatible API base URL for the LLM provider. Defaults to DeepSeek. |
| `LLM_MODEL` | No | Model name for the LLM provider. Defaults to `deepseek-chat`. |
| `DEEPSEEK_API_KEY` | Legacy | Fallback if `LLM_API_KEY` is not set (deprecated — prefer `LLM_API_KEY`). |
| `EPICODE_MASTER_KEY` | Cloud only | Master account key for cloud deployments. Grants access to account management, sub-account creation, and billing operations. |
| `EPICODE_ADMIN_KEY` | Cloud only | Administrator key for cloud deployments. Grants access to system-level operations such as user management, quota overrides, and maintenance tasks. |

## Infrastructure Settings

These variables configure the underlying infrastructure connections.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `REDIS_URL` | No | `redis://redis:6379` | Connection URL for the Redis instance. Used for caching, session storage, and rate-limiting counters in cloud deployments. |
| `EPICODE_HOST` | No | `localhost` | The hostname or IP address on which the Epicode server listens. Set to `0.0.0.0` for Docker or cloud deployments. |

## Usage Notes

### Local Development

For local development, only `EPICODE_API_KEY` and `LLM_API_KEY` are strictly required. Set them in a `.env` file or export them in your shell:

```bash
export EPICODE_API_KEY=your-local-api-key
export LLM_API_KEY=your-llm-provider-key
```

### Cloud Deployment

For cloud deployments, all authentication keys should be configured. The `EPICODE_MASTER_KEY` and `EPICODE_ADMIN_KEY` should be strong, randomly generated secrets stored in a secure vault. Example `.env` file:

```bash
LLM_API_KEY=replace-me
EPICODE_ADMIN_KEY=replace-me
EPICODE_MASTER_KEY=replace-me
REDIS_URL=redis://redis:6379
EPICODE_HOST=0.0.0.0
```

### Key Rotation

- Rotate `EPICODE_API_KEY` regularly if exposed to client-side applications.
- `EPICODE_MASTER_KEY` and `EPICODE_ADMIN_KEY` should be rotated immediately if compromised.
- `LLM_API_KEY` rotation depends on your LLM provider's account policy.

## Related Documentation

- [Deployment Guide](deployment.md) — Full deployment instructions for Docker, Kubernetes, and bare-metal.
- [API Reference](api-reference.md) — How to use the API keys in requests.
