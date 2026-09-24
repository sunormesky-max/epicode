#!/usr/bin/env python3
"""Validate the deployment configuration contract without external tooling."""

from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def read(relative_path: str) -> str:
    return (ROOT / relative_path).read_text(encoding="utf-8-sig")


def require(content: str, expected: str, source: str, failures: list[str]) -> None:
    if expected not in content:
        failures.append(f"{source}: missing {expected!r}")


def service_block(compose: str, name: str) -> str:
    lines = compose.splitlines()
    start = lines.index(f"  {name}:")
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("  ") and not lines[index].startswith("    "):
            end = index
            break
    return "\n".join(lines[start:end])


def main() -> int:
    failures: list[str] = []
    compose = read("deploy/docker-compose.yml")
    nginx = read("deploy/nginx.conf")
    kubernetes = read("deploy/kubernetes/epicode.yaml")
    values = read("deploy/helm/epicode/values.yaml")
    helm_secret = read("deploy/helm/epicode/templates/secret.yaml")
    helm_backend = read("deploy/helm/epicode/templates/backend.yaml")
    helm_frontend = read("deploy/helm/epicode/templates/frontend.yaml")
    helm_ingress = read("deploy/helm/epicode/templates/ingress.yaml")
    frontend_boot = read("frontend/api/boot.ts")
    frontend_dockerfile = read("frontend/Dockerfile")
    cloud_server = read("backend/src/bin/cloud.rs")

    required_backend = (
        "EPICODE_API_KEY",
        "EPICODE_ADMIN_KEY",
        "EPICODE_CORS_ORIGIN",
    )
    required_frontend = (
        "APP_ID",
        "APP_SECRET",
        "DATABASE_URL",
        "KIMI_AUTH_URL",
        "KIMI_OPEN_URL",
    )

    for variable in required_backend + required_frontend:
        require(compose, f"{variable}: ${{{variable}:?", "deploy/docker-compose.yml", failures)
        require(kubernetes, f"key: {variable}", "deploy/kubernetes/epicode.yaml", failures)
        require(
            helm_secret,
            f".Values.env.{variable} | required",
            "deploy/helm/epicode/templates/secret.yaml",
            failures,
        )

    for variable in required_backend:
        require(helm_backend, f"key: {variable}", "deploy/helm/epicode/templates/backend.yaml", failures)

    for variable in required_frontend:
        require(helm_frontend, f"key: {variable}", "deploy/helm/epicode/templates/frontend.yaml", failures)

    backend_compose = service_block(compose, "backend")
    frontend_compose = service_block(compose, "frontend")
    gateway_compose = service_block(compose, "gateway")
    for name, block in (("backend", backend_compose), ("frontend", frontend_compose)):
        if "\n    ports:" in block:
            failures.append(f"deploy/docker-compose.yml: {name} must not expose a host port")
    require(backend_compose, 'expose:\n      - "9111"', "deploy/docker-compose.yml", failures)
    require(frontend_compose, 'expose:\n      - "3000"', "deploy/docker-compose.yml", failures)
    require(gateway_compose, '- "8080:80"', "deploy/docker-compose.yml", failures)
    require(compose, "condition: service_healthy", "deploy/docker-compose.yml", failures)
    require(compose, 'test: ["CMD", "redis-cli", "ping"]', "deploy/docker-compose.yml", failures)

    require(nginx, "location ^~ /api/", "deploy/nginx.conf", failures)
    require(nginx, "proxy_pass http://backend:9111/;", "deploy/nginx.conf", failures)
    require(nginx, "location ^~ /api/trpc", "deploy/nginx.conf", failures)
    require(nginx, "proxy_pass http://frontend:3000;", "deploy/nginx.conf", failures)

    require(kubernetes, "name: epicode-api", "deploy/kubernetes/epicode.yaml", failures)
    require(
        kubernetes,
        'nginx.ingress.kubernetes.io/use-regex: "true"',
        "deploy/kubernetes/epicode.yaml",
        failures,
    )
    require(
        kubernetes,
        "nginx.ingress.kubernetes.io/rewrite-target: /v1/$2",
        "deploy/kubernetes/epicode.yaml",
        failures,
    )
    require(kubernetes, "path: /api/v1(/|$)(.*)", "deploy/kubernetes/epicode.yaml", failures)
    require(kubernetes, "pathType: ImplementationSpecific", "deploy/kubernetes/epicode.yaml", failures)
    require(kubernetes, "type: ClusterIP", "deploy/kubernetes/epicode.yaml", failures)
    require(kubernetes, 'command: ["redis-cli", "ping"]', "deploy/kubernetes/epicode.yaml", failures)

    require(values, "path: /api/v1(/|$)(.*)", "deploy/helm/epicode/values.yaml", failures)
    require(
        values,
        "nginx.ingress.kubernetes.io/rewrite-target: /v1/$2",
        "deploy/helm/epicode/values.yaml",
        failures,
    )
    require(values, "type: ClusterIP", "deploy/helm/epicode/values.yaml", failures)
    require(values, 'command: ["redis-cli", "ping"]', "deploy/helm/epicode/values.yaml", failures)
    require(helm_ingress, "name: {{ .Release.Name }}-api", "deploy/helm/epicode/templates/ingress.yaml", failures)
    require(
        helm_ingress,
        ".Values.ingress.api.annotations",
        "deploy/helm/epicode/templates/ingress.yaml",
        failures,
    )
    require(helm_ingress, ".Values.ingress.api.path", "deploy/helm/epicode/templates/ingress.yaml", failures)

    require(frontend_boot, 'app.get("/health"', "frontend/api/boot.ts", failures)
    require(frontend_dockerfile, "HEALTHCHECK", "frontend/Dockerfile", failures)
    require(kubernetes, "livenessProbe:", "deploy/kubernetes/epicode.yaml", failures)
    require(values, "probes:\n  backend:", "deploy/helm/epicode/values.yaml", failures)
    require(values, "  frontend:\n    liveness:", "deploy/helm/epicode/values.yaml", failures)
    require(helm_backend, 'value: "redis://{{ .Release.Name }}-redis:6379"', "deploy/helm/epicode/templates/backend.yaml", failures)
    require(cloud_server, '.route("/v1/login"', "backend/src/bin/cloud.rs", failures)
    require(cloud_server, '.route("/v1/health"', "backend/src/bin/cloud.rs", failures)
    require(cloud_server, '.route("/v1/register"', "backend/src/bin/cloud.rs", failures)
    require(cloud_server, '.route("/v1/stats/public"', "backend/src/bin/cloud.rs", failures)

    if failures:
        print("Deployment configuration validation failed:", file=sys.stderr)
        print("\n".join(f"- {failure}" for failure in failures), file=sys.stderr)
        return 1

    print("Deployment configuration validation passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
