# Epicode Helm Chart

This chart deploys Epicode (backend + frontend + optional Redis) on Kubernetes.

## Quick start

```bash
# Add required values
cat > my-values.yaml <<EOF
env:
  EPICODE_API_KEY: "$(openssl rand -base64 32)"
  EPICODE_ADMIN_KEY: "$(openssl rand -base64 32)"
  EPICODE_CORS_ORIGIN: "https://epicode.example.com"
  DEEPSEEK_API_KEY: "your-deepseek-key"
  APP_ID: "your-app-id"
  APP_SECRET: "your-app-secret"
  DATABASE_URL: "your-database-url"
  KIMI_AUTH_URL: "https://auth.example.com"
  KIMI_OPEN_URL: "https://open.example.com"
ingress:
  hosts:
    - host: epicode.example.com
      paths:
        - path: /api/trpc
          pathType: Prefix
          service: frontend
        - path: /api/health
          pathType: Exact
          service: backend
        - path: /api/register
          pathType: Exact
          service: backend
        - path: /api/mcp
          pathType: Exact
          service: backend
        - path: /api/stats/public
          pathType: Exact
          service: backend
        - path: /docs
          pathType: Exact
          service: backend
        - path: /openapi.yaml
          pathType: Exact
          service: backend
        - path: /health
          pathType: Exact
          service: backend
        - path: /stats/public
          pathType: Exact
          service: backend
        - path: /
          pathType: Prefix
          service: frontend
  tls:
    - secretName: epicode-tls
      hosts:
        - epicode.example.com
EOF

# Install
helm install epicode deploy/helm/epicode -f my-values.yaml

# Verify
kubectl get pods -l app.kubernetes.io/instance=epicode
curl https://epicode.example.com/health
```

The chart creates a separate NGINX API Ingress that rewrites
`/api/v1/...` to the Cloud backend's `/v1/...` route. Keep
`ingress.className` set to `nginx`, or provide an equivalent rewrite for your
Ingress controller. Backend and frontend Services remain `ClusterIP`; the
Ingress is the only public entry point.

## Configuration

| Key | Description | Default |
|-----|-------------|---------|
| `replicaCount.backend` | Backend replicas | `1` |
| `replicaCount.frontend` | Frontend replicas | `2` |
| `image.backend.repository` | Backend image | `ghcr.io/sunormesky-max/epicode-backend` |
| `image.backend.tag` | Backend tag | `1.0.1` |
| `image.frontend.repository` | Frontend image | `ghcr.io/sunormesky-max/epicode-frontend` |
| `image.frontend.tag` | Frontend tag | `1.0.1` |
| `env.EPICODE_API_KEY` | **Required** Cloud backend security key | `""` |
| `env.EPICODE_ADMIN_KEY` | **Required** Admin API key | `""` |
| `env.EPICODE_CORS_ORIGIN` | **Required** public browser origin | `""` |
| `env.APP_ID` | **Required** frontend application ID | `""` |
| `env.APP_SECRET` | **Required** frontend application secret | `""` |
| `env.DATABASE_URL` | **Required** frontend database URL | `""` |
| `env.KIMI_AUTH_URL` | **Required** frontend authentication service URL | `""` |
| `env.KIMI_OPEN_URL` | **Required** frontend open-platform service URL | `""` |
| `env.EPICODE_MASTER_KEY` | Optional 32-byte base64 encryption key | `""` |
| `env.DEEPSEEK_API_KEY` | DeepSeek LLM key (optional) | `""` |
| `env.REDIS_URL` | Optional external Redis URL; bundled Redis is used when enabled | `""` |
| `persistence.enabled` | Enable PVC for backend data | `true` |
| `persistence.size` | PVC size | `10Gi` |
| `ingress.enabled` | Enable Ingress | `true` |
| `ingress.className` | Ingress class | `nginx` |
| `redis.enabled` | Deploy Redis sidecar | `true` |
| `autoscaling.enabled` | Enable HPA | `false` |

## TLS

The chart annotations default to `cert-manager.io/cluster-issuer: letsencrypt-prod`. Install [cert-manager](https://cert-manager.io/) first:

```bash
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.16.0/cert-manager.yaml
```

## Upgrading

```bash
helm upgrade epicode deploy/helm/epicode -f my-values.yaml
```

## Uninstalling

```bash
helm uninstall epicode
# PVC is retained by default; delete manually if desired:
kubectl delete pvc epicode-data
```
