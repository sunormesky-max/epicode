<div align="center">

# Epicode

### AI 记忆操作系统

**Give AI an Unforgettable Memory**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88+-orange.svg)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev/)
[![Live](https://img.shields.io/badge/Live-epicode.cn-success.svg)](https://epicode.cn)

[官网](https://epicode.cn) · [快速上手](#-快速上手) · [文档](https://epicode.cn/#/docs) · [OpenAPI](backend/docs/openapi.yaml) · [示例](docs/examples.md) · [部署](docs/deployment.md) · [API 参考](#-api-参考) · [架构](#-架构) · [变更记录](CHANGELOG.md) · [行为规范](CODE_OF_CONDUCT.md)

</div>

---

## Epicode 是什么？

Epicode 是一个**开源的空间 AI 记忆系统**。它将 AI 的记忆以正四面体的形式存储在连续三维空间中，通过知识图谱自动提取关系，为 AI 代理提供跨会话的持久化记忆能力。

**核心特性：**

- **向量记忆存储** — 每条信息被嵌入、索引，在跨会话中可检索
- **语义搜索** — BM25 + HNSW 混合搜索，自然语言查询返回上下文相关结果
- **知识图谱** — 自动关系提取，创建互联记忆的动态图谱
- **MCP 集成** — 35 个标准化工具，任何 AI 代理都能存储、搜索和回忆
- **SMRP 规范** — 结构化记忆响应信封，暴露 tier、source、topology 和写入副产物
- **多用户 Cloud** — 完整的用户注册、认证、配额管理、邀请码系统
- **自动防御** — 内置服务器安全守护，SSH/Web/蜜罐多层防护

---

## 网站导航

**在线体验：[epicode.cn](https://epicode.cn)**

### 公共页面

| 页面 | 路径 | 说明 |
|------|------|------|
| 首页 | `/#/` | 产品介绍、四大核心能力、快速上手代码示例、系统技能展示 |
| 文档 | `/#/docs` | API 文档、MCP 协议说明、端点列表与请求/响应示例 |
| 快速上手 | `/#/guide` | 分步教程，几分钟内集成 Epicode 到你的 AI 代理 |
| 社区 | `/#/community` | 社区技能浏览与搜索 |
| 基准测试 | `/#/benchmarks` | 性能基准测试数据与可视化图表 |
| 注册 | `/#/register` | 创建账户，获取 API Key |
| 登录 | `/#/login` | 登录控制台 |

### 仪表盘（需登录）

| 页面 | 路径 | 说明 |
|------|------|------|
| 总览 | `/#/dashboard` | 记忆总量、簇数、能量、图表分析、聚类分布 |
| 记忆管理 | `/#/dashboard/memories` | 多维筛选（标签/分类/时间）、搜索、详情查看 |
| 知识图谱 | `/#/dashboard/graph` | 交互式力导向图、节点拖拽/缩放、关系详情面板 |
| 技能管理 | `/#/dashboard/skills` | 系统技能与社区技能浏览、多维筛选 |
| 子账户 | `/#/dashboard/accounts` | 子账户管理、资源用量、权限控制（仅主账户可见） |

---

## 项目结构

```
epicode/
├── frontend/          # React 前端（TypeScript + Vite 7 + Tailwind）
│   ├── src/           #   页面、组件、API 层、国际化
│   ├── api/           #   Hono SSR 后端（tRPC + Drizzle ORM）
│   └── db/            #   数据库 Schema 与迁移
├── backend/           # Rust 后端（Axum + Tokio + ONNX）
│   ├── src/domain/    #   几何原语：顶点、四面体、空间索引、圆柱体
│   ├── src/engine/    #   38 个业务模块：网关、认知、调度、知识图谱、HNSW…
│   ├── src/api/       #   HTTP 路由与中间件
│   └── src/bin/       #   cloud.rs（Cloud 服务器入口）
└── guard/             # 自动防御系统（Rust 单文件）
    ├── src/main.rs    #   SSH/Web/蜜罐检测 + nftables 封禁
    └── install.sh     #   部署脚本
```

---

## 快速上手

### 1. 注册获取 API Key

访问 [epicode.cn/#/register](https://epicode.cn/#/register) 注册账户，获取你的 API Key。

### 2. 存储一条记忆

```bash
curl -X POST https://epicode.cn/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"content": "Epicode 是一个空间 AI 记忆系统", "labels": ["project", "ai"]}'
```

### 3. 搜索记忆

```bash
curl -X POST https://epicode.cn/api/v1/search \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"query": "AI 记忆"}'
```

### 4. 查看知识图谱

```bash
curl -H "X-API-Key: your-api-key" \
  https://epicode.cn/api/v1/graph/analysis
```

---

## API 参考

> 线上站点通常通过 `/api` 前缀暴露；本地部署或反向代理配置不同，可能直接使用 `/v1`。

### 核心端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/v1/remember` | 存储新记忆 |
| `POST` | `/v1/search` | 语义搜索记忆 |
| `POST` | `/v1/recall` | 深度回忆（搜索 + 知识图谱展开） |
| `GET` | `/v1/stats` | 获取空间统计 |
| `GET` | `/v1/graph/analysis` | 知识图谱分析 |
| `GET` | `/v1/health` | 健康检查 |

### MCP 工具（35 个）

通过 MCP（Model Context Protocol）协议，支持以下操作：

`memory_create` · `memory_search` · `memory_recall` · `memory_get` · `memory_list` · `memory_update` · `memory_delete` · `ctx_load` · `ctx_save` · `pattern_learn` · `pattern_recall` · `decision_record` · `bug_memory` · `session_summary` · `space_stats` · `dream_cycle` · `knowledge_relations` · `concepts` · `context_observe` · `identity_confirm` · `skill_execute` · `feedback_submit` · `skills_sync` · `enforced_rules` · `project_list` · `identity_step` · `identity_finalize`

### SMRP 结构化响应

SMRP（Structured Memory Response Protocol）为记忆类工具提供统一的响应信封：

```json
{
  "protocol": {
    "schema_version": "1.0",
    "tool": "memory_search",
    "ok": true,
    "error": null
  },
  "data": {},
  "status": {}
}
```

它把记忆结果从“扁平列表”提升为“可解释结构”，为 AI 代理提供 tier、source、topology 和 placement 等信息。

---

## SDK、示例与部署资产

- **OpenAPI 规范**：`backend/docs/openapi.yaml`，Cloud 部署后可直接访问 `/docs` 与 `/openapi.yaml`
- **官方 SDK**：`backend/sdk/python`、`backend/sdk/typescript`
- **快速示例**：见 `examples/`，包含 `curl`、Node.js、Python 三种接入方式
- **部署模板**：见 `deploy/docker-compose.yml` 与 `deploy/kubernetes/epicode.yaml`
- **运维文档**：见 `docs/deployment.md` 与 `docs/troubleshooting.md`

---

## 架构

### 数据流

```
AI 代理 → POST /remember → 安全中间件（API Key + 速率限制 + 能量检查）
    → GatewayCenter（嵌入计算 → LLM 分类 → 空间放置）
    → 新四面体放入 Space（自动合并近距离顶点 → 自然形成簇）
    → 知识图谱更新
    → 调度器周期运行：自动脉冲 / 自动链接 / 去重 / 梦境循环
```

### 空间模型

记忆以**正四面体**（统一边长 1.0）的形式存储在三维空间中：

- 共享顶点的四面体自然聚簇为多面体
- 中央空心圆柱体作为系统枢纽，四层结构：本能 → 认知 → 服务 → 身份
- 端口通过星型拓扑与外部多面体簇相连
- 脉冲沿拓扑传播：本能层端口 → 多面体簇 → 同一端口返回

### 并发模型

- 领域状态（Space、Cylinder、KnowledgeGraph）使用 `RwLock` 内部可变性
- 引擎子系统通过异步 `broadcast::EventBus` 通信
- 后台任务通过 `tokio::task` 运行

---

## 本地开发

### 测试环境

| 项目 | 配置 |
|------|------|
| 服务器 | 2 vCPU / 4GB RAM |
| 嵌入模型 | ONNX Runtime（进程内） |
| 存储引擎 | SQLite + HNSW 索引 |
| 运行时 | Rust / Tokio Async |

### 性能基准测试（来自 [epicode.cn/benchmarks](https://epicode.cn/#/benchmarks)）

#### API 延迟（ms）

| 操作 | P50 | P95 | P99 |
|------|-----|-----|-----|
| `remember` | 45 | 120 | 280 |
| `search` | 38 | 95 | 210 |
| `recall` | 120 | 350 | 680 |
| `timeline` | 12 | 28 | 55 |
| `graph/export` | 85 | 240 | 520 |
| `stats` | 8 | 18 | 35 |

#### 吞吐量随数据量变化

| 记忆数 | QPS | P50 延迟 |
|--------|-----|----------|
| 100 | 85 | 42ms |
| 1,000 | 78 | 47ms |
| 10,000 | 65 | 58ms |
| 50,000 | 52 | 68ms |
| 100,000 | 41 | 82ms |

#### 嵌入批处理性能

| Batch Size | 吞吐量 (embed/s) | 延迟 |
|------------|------------------|------|
| 1 | 12 | 42ms |
| 10 | 85 | 58ms |
| 50 | 240 | 105ms |
| 100 | 350 | 145ms |

#### 知识图谱扩展性（ms）

| 图节点数 | 图构建 | 搜索 | 回忆 |
|----------|--------|------|------|
| 100 | 50 | 15 | 60 |
| 1,000 | 350 | 30 | 140 |
| 10,000 | 2,800 | 78 | 580 |
| 50,000 | 8,500 | 145 | 1,200 |

### 前端

```bash
cd frontend
npm install
npm run dev          # 开发服务器 :5173
npm run build        # 生产构建
```

### 后端

```bash
cd backend

# Windows
cargo.bat build              # debug 构建
cargo.bat build --release    # release 构建
cargo.bat run                # 启动服务器 :9110
cargo.bat test --lib         # 运行测试

# Linux（生产部署）
cargo build --release
./target/release/epicode --cloud   # Cloud 模式 :9111
```

### 防御系统

```bash
cd guard
cargo build --release
# 部署到服务器
sudo cp target/release/epicode-guard /usr/local/bin/
sudo cp epicode-guard.service /etc/systemd/system/
sudo systemctl enable --now epicode-guard
```

### 环境变量

| 变量 | 必需 | 说明 |
|------|------|------|
| `TETRAMEM_API_KEY` | 是 | API 请求认证密钥 |
| `DEEPSEEK_API_KEY` | 是 | LLM 认知功能（分类、推理、别名生成） |
| `TETRAMEM_MASTER_KEY` | Cloud | 主账户密钥 |
| `TETRAMEM_ADMIN_KEY` | Cloud | 管理员操作密钥 |

---

## 技术栈

| 层 | 技术 |
|----|------|
| **前端** | React 19 · TypeScript · Vite 7 · Tailwind CSS · Framer Motion · Recharts · Radix UI |
| **后端** | Rust · Axum · Tokio · SQLite (rusqlite) · ONNX Runtime (ort) · HNSW |
| **嵌入** | ONNX 模型（进程内） · HTTP 嵌入降级方案 |
| **认知** | DeepSeek LLM API（工具调用） |
| **防御** | Rust · nftables · firewalld · TCP 蜜罐 |
| **部署** | Nginx · Let's Encrypt · systemd · Docker |

---

## 安全

- API Key 认证 + 管理员 Key 双层体系
- 速率限制 + 能量令牌桶
- Nginx 安全头（HSTS、X-Frame-Options、CSP）
- epicode-guard 自动防御：SSH 暴力破解检测、62 种 Web 攻击模式、8 端口 TCP 蜜罐、连接洪水检测、文件完整性监控

---

## Contributing

We welcome contributions! Please see:

- [Contributing Guide](CONTRIBUTING.md) — development setup, code style, PR guidelines
- [Code of Conduct](CODE_OF_CONDUCT.md) — community behavior expectations
- [Security Policy](SECURITY.md) — how to report vulnerabilities
- [Discussions](https://github.com/sunormesky-max/epicode/discussions) — ask questions, share ideas

## License

本项目采用 [MIT](LICENSE) 许可证。

---

<div align="center">

**Made with ❤️ by [刘启航](https://github.com/sunormesky-max)**

</div>
