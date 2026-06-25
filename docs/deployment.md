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
| `EPICODE_ADMIN_KEY` | Cloud admin surface |
| `EPICODE_MASTER_KEY` | Optional master encryption key |
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

## TLS / HTTPS

Terminate TLS at one of these layers (pick one — do not double-terminate):

| Layer | Tool | Notes |
|-------|------|-------|
| Load balancer | AWS ALB, GCP HTTPS LB, Cloudflare | Easiest; cert managed by cloud provider |
| Ingress controller | nginx-ingress, traefik, Caddy | Use `cert-manager` for Let's Encrypt |
| Nginx gateway | the bundled `deploy/nginx.conf` | Add `listen 443 ssl;` + cert paths |

Minimum config for the bundled nginx:

```nginx
server {
    listen 443 ssl http2;
    server_name epicode.example.com;

    ssl_certificate     /etc/ssl/epicode/fullchain.pem;
    ssl_certificate_key /etc/ssl/epicode/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    # ... existing location blocks ...
}

server {
    listen 80;
    server_name epicode.example.com;
    return 301 https://$host$request_uri;
}
```

## Persistent volumes

| Service | Mount path | Purpose | Backup cadence |
|---------|-----------|---------|----------------|
| backend | `/app/data` | SQLite DB, ONNX models, backups | Daily (or before upgrades) |
| redis | `/data` | AOF/RDB persistence (if enabled) | Optional (cache is rebuildable) |

Kubernetes example:

```yaml
spec:
  containers:
    - name: backend
      volumeMounts:
        - name: data
          mountPath: /app/data
  volumes:
    - name: data
      persistentVolumeClaim:
        claimName: epicode-backend-data
```

## Monitoring

- **Health check**: `GET /health` (no auth) — returns `{"status":"ok"}` and is suitable for liveness/readiness probes
- **Public stats**: `GET /api/v1/stats/public` — lightweight metrics without auth
- **Logs**: backend emits structured logs via `tracing`; set `RUST_LOG=info` (or `debug` for troubleshooting)
- **Metrics endpoint**: planned; for now scrape `/api/v1/stats` with auth

Kubernetes probes example:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 9111
  initialDelaySeconds: 10
  periodSeconds: 30
readinessProbe:
  httpGet:
    path: /health
    port: 9111
  initialDelaySeconds: 5
  periodSeconds: 10
```

## Upgrade & rollback

1. **Backup**: `cp -r backend/data backend/data.backup-$(date +%F)`
2. **Pull new image**: `docker compose pull` (or update tag in K8s manifest)
3. **Rolling update**: `docker compose up -d` (or `kubectl rollout restart deployment/epicode-backend`)
4. **Verify**: `curl https://your-host/health` and `curl https://your-host/api/v1/stats/public`
5. **Rollback** if needed:
   - Docker Compose: revert image tag, `docker compose up -d`
   - Kubernetes: `kubectl rollout undo deployment/epicode-backend`

## Multi-tenant notes

- Cloud mode (`epicode-cloud` binary) enforces per-tenant isolation via `EPICODE_ADMIN_KEY` + API key scoping
- Each tenant has its own encryption context derived from the master key
- Rate limits are per-tenant; configure via `REDIS_URL` for distributed limiting
- The `guard` daemon is optional and only relevant for self-hosted single-tenant deployments

## API prefix reference

| Access path | Base URL |
|------------|----------|
| Through Nginx (public) | `https://epicode.cn/api/v1` |
| Direct backend (cloud) | `http://localhost:9111/v1` |
| Direct backend (single-tenant) | `http://localhost:9110/v1` |
| Health (either) | `http://localhost:9111/health` or `http://localhost:9110/v1/health` |
