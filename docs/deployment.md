# Deployment Guide

Epicode now ships with both **Docker Compose** and **Kubernetes** deployment assets.

## What is included

- `deploy/docker-compose.yml` — local or single-host deployment
- `deploy/nginx.conf` — reverse proxy that routes Cloud `/api/*` requests to the Rust backend, `/api/trpc` to the frontend, and `/` to the frontend
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
| frontend | 3000 | none (gateway only) |
| backend | 9111 | none (gateway only) |
| nginx gateway | 80 | 8080 |

After startup:

- unified gateway: `http://localhost:8080`
- backend health through gateway: `http://localhost:8080/health`
- Swagger UI through gateway: `http://localhost:8080/docs`

The backend, frontend, and Redis containers are intentionally private to the
Compose network. Use `docker compose exec backend curl -fsS
http://localhost:9111/health` when debugging the backend directly.

## Production environment contract

| Variable | Purpose |
| --- | --- |
| `EPICODE_API_KEY` | **Required** Cloud backend security key; generate with `openssl rand -base64 32` |
| `EPICODE_ADMIN_KEY` | **Required** Cloud admin-surface key |
| `EPICODE_CORS_ORIGIN` | **Required** public origin, such as `https://epicode.example.com` or `http://localhost:8080` |
| `APP_ID` | **Required** frontend production application ID |
| `APP_SECRET` | **Required** frontend production application secret |
| `DATABASE_URL` | **Required** frontend production database URL |
| `KIMI_AUTH_URL` | **Required** frontend authentication service URL |
| `KIMI_OPEN_URL` | **Required** frontend open-platform service URL |
| `EPICODE_MASTER_KEY` | Optional master encryption key |
| `DEEPSEEK_API_KEY` | Optional LLM-backed ask/recall key |
| `REDIS_URL` | Optional external L2 cache URL; defaults to the bundled Redis service |
| `OWNER_UNION_ID` | Optional frontend owner identifier |

`deploy/.env.example` contains the complete Compose contract. Replace every
`replace-me` value before starting a production deployment; Compose fails fast
when any required variable is missing.

## Kubernetes

The manifest assumes:

1. an **NGINX Ingress Controller** is already installed
2. the backend and frontend images are published
3. every value in the `epicode-secrets` Secret template is replaced

Apply:

```bash
kubectl apply -f deploy/kubernetes/epicode.yaml
```

The ingress routes:

- `/api/v1/*` → backend `/v1/*` (the API Ingress strips `/api`)
- `/api/health`, `/api/register`, and `/api/mcp` → corresponding backend routes
- `/api/trpc` → frontend
- `/docs`, `/openapi.yaml`, `/health`, `/stats/public` → backend
- `/` → frontend

The API rewrite uses the NGINX Ingress annotations
`nginx.ingress.kubernetes.io/use-regex` and
`nginx.ingress.kubernetes.io/rewrite-target`. Deploying this manifest with a
different controller requires an equivalent rewrite from `/api/...` to `/...`.
Both application Services are `ClusterIP`; only the Ingress is public.

## Image build notes

- backend images now build against **Rust 1.88**
- frontend images no longer contain machine-specific proxy settings
- backend container entrypoint is `epicode-cloud`

## Recommended production setup

1. terminate TLS at ingress or a managed load balancer
2. set `REDIS_URL` when enabling the query cache beyond local memory
3. persist `/app/data` for the backend
4. keep frontend and backend on the same public host so `/api/*` works without extra client changes
5. set `EPICODE_CORS_ORIGIN` to that exact public origin

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

## Health checks

- **Backend health check**: `GET /health` (no auth) — returns `{"status":"ok"}` and is used for liveness/readiness probes
- **Frontend health check**: `GET /health` on the frontend container is used only by its container/pod probes; the public gateway's `/health` remains the backend health endpoint
- **Public stats**: `GET /api/v1/stats/public` — lightweight metrics without auth
- **Logs**: backend emits structured logs via `tracing`; set `RUST_LOG=info` (or `debug` for troubleshooting)
- **Metrics endpoint**: planned; for now scrape `/api/v1/stats` with auth

Compose waits for the backend, frontend, and Redis health checks before
starting the gateway. Kubernetes and Helm use the same `/health` HTTP probes
for application containers and `redis-cli ping` for Redis.

Kubernetes backend probe example:

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
| Through gateway or NGINX Ingress (public) | `https://epicode.cn/api/v1` |
| Direct backend (cloud) | `http://localhost:9111/v1` |
| Direct backend (single-tenant) | `http://localhost:9110/v1` |
| Health (either) | `http://localhost:9111/health` or `http://localhost:9110/v1/health` |
