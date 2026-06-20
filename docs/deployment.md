# Deployment Guide

Epicode now ships with both **Docker Compose** and **Kubernetes** deployment assets.

## What is included

- `deploy/docker-compose.yml` — local or single-host deployment
- `deploy/nginx.conf` — reverse proxy that routes `/api/*` to the Rust backend and `/` to the frontend
- `deploy/.env.example` — required environment variables
- `deploy/kubernetes/epicode.yaml` — namespace, secret template, services, deployments, ingress

## Docker Compose

```bash
cd deploy
cp .env.example .env
docker compose up --build -d
```

Default exposed ports:

| Service | Internal port | External port |
| --- | --- | --- |
| frontend | 3000 | 3000 |
| backend | 9111 | 9111 |
| nginx gateway | 80 | 8080 |

After startup:

- frontend: `http://localhost:3000`
- backend health: `http://localhost:9111/health`
- unified gateway: `http://localhost:8080`
- Swagger UI through gateway: `http://localhost:8080/docs`

## Required environment variables

| Variable | Purpose |
| --- | --- |
| `DEEPSEEK_API_KEY` | LLM-backed ask/recall flows |
| `TETRAMEM_ADMIN_KEY` | Cloud admin surface |
| `TETRAMEM_MASTER_KEY` | Optional master encryption key |
| `REDIS_URL` | Optional L2 cache backend |
| `EPICODE_HOST` | Hostname used by ingress / reverse proxy |

## Kubernetes

The manifest assumes:

1. an ingress controller is already installed
2. the backend and frontend images are published
3. secrets are supplied through the `epicode-secrets` Secret

Apply:

```bash
kubectl apply -f deploy/kubernetes/epicode.yaml
```

The ingress routes:

- `/api/*`, `/docs`, `/openapi.yaml`, `/health` → backend
- `/` → frontend

## Image build notes

- backend images now build against **Rust 1.88**
- frontend images no longer contain machine-specific proxy settings
- backend container entrypoint is `epicode-cloud`

## Recommended production setup

1. terminate TLS at ingress or a managed load balancer
2. set `REDIS_URL` when enabling the query cache beyond local memory
3. persist `/app/data` for the backend
4. keep frontend and backend on the same public host so `/api/*` works without extra client changes
