<div align="center">

# Epicode

## 给 AI 一个难忘的记忆

[![CI](https://github.com/sunormesky-max/epicode/actions/workflows/ci.yml/badge.svg)](https://github.com/sunormesky-max/epicode/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/github/v/release/sunormesky-max/epicode)](https://github.com/sunormesky-max/epicode/releases)
[![Docker](https://img.shields.io/badge/Docker-ready-2496ED?logo=docker)](deploy/docker-compose.yml)
[![Rust](https://img.shields.io/badge/Rust-1.88+-orange?logo=rust)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev/)

[![GitHub stars](https://img.shields.io/github/stars/sunormesky-max/epicode?style=social)](https://github.com/sunormesky-max/epicode/stargazers)
[![GitHub Discussions](https://img.shields.io/github/discussions/sunormesky-max/epicode)](https://github.com/sunormesky-max/epicode/discussions)
[![Docs](https://img.shields.io/badge/Docs-epicode.cn-success)](https://epicode.cn/#/docs)
[![Live Demo](https://img.shields.io/badge/Live-epicode.cn-2ea44f)](https://epicode.cn)

[English](README.md) · [中文](README.zh.md) · [快速开始](#快速开始) · [文档](docs/) · [OpenAPI](backend/docs/openapi.yaml) · [Releases](https://github.com/sunormesky-max/epicode/releases)

</div>

---

Epicode 是一个**开源的空间 AI 记忆系统**。它将 AI 的记忆以正四面体形式存储在连续三维空间中，自动提取关系构建知识图谱，为 AI 代理提供跨会话的持久化记忆能力。

## 快速开始

用 Docker Compose 本地启动是最快的方式：

```bash
git clone https://github.com/sunormesky-max/epicode.git
cd epicode/deploy
cp .env.example .env
# 编辑 .env，填入 DEEPSEEK_API_KEY 和其他密钥
docker compose up --build -d
```

然后存储并搜索一条记忆：

```bash
curl -X POST http://localhost:8080/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"content": "Epicode 让 AI 拥有持久化的空间记忆", "labels": ["ai", "memory"]}'

curl -X POST http://localhost:8080/api/v1/search \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"query": "AI 记忆"}'
```

> 💡 **在线演示：** [epicode.cn](https://epicode.cn) · 仪表盘截图将在后续 PR 中补充。

## 核心特性

- **空间记忆** — 以三维空间中的正四面体存储记忆，实现自然聚簇。
- **语义搜索** — BM25 + HNSW 混合搜索，支持自然语言查询。
- **知识图谱** — 自动关系提取，动态更新图谱。
- **MCP 集成** — 35 个标准化工具，任何 MCP 兼容的 AI 代理都能使用。
- **SMRP 协议** — 结构化记忆响应，暴露拓扑与位置元数据。
- **多租户 Cloud** — 用户管理、配额、邀请码和管理员控制。
- **自托管防御** — `epicode-guard` 监控 SSH/Web/蜜罐流量并自动封禁攻击者。

## 架构概览

```text
AI 代理 → POST /remember
    → 安全中间件（API Key + 速率限制 + 能量检查）
    → GatewayCenter（嵌入计算 → LLM 分类 → 空间放置）

## SDK

### Python

```bash
pip install epicode-sdk
```

```python
from epicode import EpicodeClient

client = EpicodeClient("your-api-key")
client.remember("项目截止日期是6月15日")
results = client.search("截止日期")
```

### TypeScript / JavaScript

```bash
npm install epicode-sdk
```

```typescript
import { EpicodeClient } from "epicode-sdk";

const client = new EpicodeClient("your-api-key");
await client.remember("已部署 v2.3 到生产环境");
const results = await client.search("生产部署");
```

> **注意：** 旧包名 `tetramem-sdk` 已弃用，请使用 `epicode-sdk`。

    → 新四面体放入 Space（自动合并近距离顶点）
    → 知识图谱更新
    → 调度器后台运行：脉冲 / 自动链接 / 去重 / 梦境循环
```

更多细节见 [docs/architecture.md](docs/architecture.md)（英文）。

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端 | React 19 · TypeScript · Vite 7 · Tailwind CSS |
| 后端 | Rust · Axum · Tokio · SQLite · ONNX Runtime |
| 搜索 | HNSW · BM25 · ONNX 嵌入 |
| 认知 | DeepSeek LLM API |
| 防御 | Rust · nftables · firewalld · TCP 蜜罐 |
| 部署 | Docker · Docker Compose · Kubernetes · Nginx |

## 本地开发

```bash
# 前端
cd frontend
npm install
npm run dev        # http://localhost:5173

# 后端
cd backend
cargo build --release
cargo test --all-targets
./target/release/epicode --cloud   # Cloud 模式 :9111
```

完整开发环境配置见 [CONTRIBUTING.md](CONTRIBUTING.md)（英文）。

## Docker 部署

```bash
cd deploy
cp .env.example .env
# 填写 DEEPSEEK_API_KEY、TETRAMEM_ADMIN_KEY、TETRAMEM_MASTER_KEY
docker compose up --build -d
```

访问 `http://localhost:8080`。生产部署细节见 [docs/deployment.md](docs/deployment.md)（英文）。

## 文档

- [Architecture](docs/architecture.md) — 数据流、空间模型、并发模型（英文）。
- [API Reference](docs/api-reference.md) — HTTP 端点与 MCP 工具（英文）。
- [MCP Protocol](docs/mcp-protocol.md) — SMRP 信封与代理集成（英文）。
- [Configuration](docs/configuration.md) — 环境变量与密钥（英文）。
- [Benchmarks](docs/benchmarks.md) — 性能数据与硬件需求（英文）。
- [Deployment](docs/deployment.md) — Docker、Kubernetes 与裸机部署（英文）。
- [Examples](docs/examples.md) — curl、Node.js、Python 示例（英文）。
- [Troubleshooting](docs/troubleshooting.md) — 常见问题与排查（英文）。

## 社区与贡献

欢迎贡献！

- [Discussions](https://github.com/sunormesky-max/epicode/discussions) — 提问和交流想法。
- [Issues](https://github.com/sunormesky-max/epicode/issues) — Bug 报告和功能请求。
- [Contributing Guide](CONTRIBUTING.md) — 开发环境、提交规范、PR 流程（英文）。
- [Security Policy](SECURITY.md) — 安全漏洞私下报告方式。
- [Roadmap](ROADMAP.md) — 即将推出的功能和长期规划（英文）。

## 许可证

Epicode 采用 [MIT License](LICENSE) 开源许可证。

---

<div align="center">

[![Star History Chart](https://api.star-history.com/svg?repos=sunormesky-max/epicode&type=Date)](https://star-history.com/#sunormesky-max/epicode&Date)

**Made with ❤️ by [sunormesky-max](https://github.com/sunormesky-max) and contributors.**

</div>
