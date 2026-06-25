# Epicode Helm Chart

This chart deploys Epicode (backend + frontend + optional Redis) on Kubernetes.

## Quick start

```bash
# Add required values
cat > my-values.yaml <<EOF
env:
  EPICODE_ADMIN_KEY: "$(openssl rand -base64 32)"
  EPICODE_MASTER_KEY: "$(openssl rand -base64 32)"
  DEEPSEEK_API_KEY: "your-deepseek-key"
ingress:
  hosts:
    - host: epicode.example.com
      paths:
        - path: /api
          pathType: Prefix
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

## Configuration

| Key | Description | Default |
|-----|-------------|---------|
| `replicaCount.backend` | Backend replicas | `1` |
| `replicaCount.frontend` | Frontend replicas | `2` |
| `image.backend.repository` | Backend image | `ghcr.io/sunormesky-max/epicode-backend` |
| `image.backend.tag` | Backend tag | `1.0.1` |
| `image.frontend.repository` | Frontend image | `ghcr.io/sunormesky-max/epicode-frontend` |
| `image.frontend.tag` | Frontend tag | `1.0.1` |
| `env.EPICODE_ADMIN_KEY` | **Required** Admin API key | `""` |
| `env.EPICODE_MASTER_KEY` | **Required** 32-byte base64 encryption key | `""` |
| `env.DEEPSEEK_API_KEY` | DeepSeek LLM key (optional) | `""` |
| `env.REDIS_URL` | Redis URL (optional) | `""` |
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
