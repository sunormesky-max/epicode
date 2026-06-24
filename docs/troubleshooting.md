# Troubleshooting

## Rust build fails because of compiler version

Epicode currently requires **Rust 1.88** for the backend build and CI toolchain.

Check:

```bash
rustc --version
```

If needed:

```bash
rustup toolchain install 1.88.0
rustup override set 1.88.0
```

## Frontend can load but API calls return 404

The frontend expects the public API under `/api/*`.

Make sure your reverse proxy or ingress sends:

- `/api/*` → backend
- `/docs`, `/openapi.yaml`, `/health` → backend
- everything else → frontend

`deploy/nginx.conf` and `deploy/kubernetes/epicode.yaml` already implement this layout.

## `ask` or recall quality is degraded

Check these first:

1. `DEEPSEEK_API_KEY` is set
2. ONNX model files are present for the backend image or local runtime
3. the backend can write to its data directory

## Container starts but healthcheck fails

The cloud backend listens on **9111** by default in containerized deployments.

Verify:

```bash
curl http://localhost:9111/health
```

If you override `EPICODE_LISTEN_ADDR`, keep the healthcheck, ingress, and port mappings aligned.

## Cache features do not use Redis

That is expected unless `REDIS_URL` is set. Without it, Epicode still uses the in-process L1 cache.

## Swagger UI is missing

Swagger UI is exposed by the backend at:

- `/docs`
- `/openapi.yaml`

If those routes are unavailable through your public host, fix proxy routing first.
