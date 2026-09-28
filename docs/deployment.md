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

Default exposed ports (backend/frontend remain bound to 127.0.0.1; the gateway
is HTTP-only and is also bound to loopback. Do not publish these HTTP ports
directly to the public internet or treat the bundled gateway as a TLS endpoint):

| Service | Internal port | External binding |
| --- | --- | --- |
| nginx gateway | 80 | `127.0.0.1:8080`（仅本机；需外置 TLS 代理） |
| backend | 9111 | `127.0.0.1:9111`（仅本机） |
| frontend | 3000 | `127.0.0.1:3000`（仅本机） |

After startup:

- unified gateway: `http://127.0.0.1:8080`（仅本机 HTTP）
- Swagger UI through gateway: `http://127.0.0.1:8080/docs`
- backend health（本机调试）: `http://localhost:9111/health`

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

1. terminate TLS in a host-level reverse proxy before forwarding to `127.0.0.1:8080`
2. set `REDIS_URL` when enabling the query cache beyond local memory
3. persist `/app/data` for the backend
4. keep frontend and backend on the same public host so `/api/*` works without extra client changes

## TLS / HTTPS

The bundled gateway only listens for plaintext HTTP. Compose binds it to
`127.0.0.1:8080` and does not provision a certificate or publish port 443.
Before serving public traffic, run a TLS-terminating proxy on the Docker host
and forward requests to the loopback gateway. For example, a host-level Nginx
can use:

```nginx
server {
    listen 443 ssl;
    server_name epicode.example.com;

    # Provision these files through your certificate manager; they are not
    # included in this repository or the Compose deployment.
    ssl_certificate     /etc/letsencrypt/live/epicode.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/epicode.example.com/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }
}

server {
    listen 80;
    server_name epicode.example.com;
    return 301 https://$host$request_uri;
}
```

If the TLS proxy runs on another host or in a separate network namespace, bind
the gateway to a private interface instead of loopback and restrict that port
with a firewall. Never expose the gateway's HTTP port publicly.

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
