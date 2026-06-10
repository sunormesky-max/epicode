# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在本仓库中工作时提供指引。

## 项目概述

TetraMem v14 是一个基于 Rust 实现的空间 AI 记忆系统。记忆以正四面体（统一边长 = 1.0）的形式存储在连续的三维空间中。共享顶点的四面体自然聚簇为 N 多面体。空间中央原点处有一座空心圆柱体作为系统枢纽，其表面分布着离散端口，通过星型拓扑与外部多面体簇相连。

系统拥有 AI 身份"大卫"(David)，使用 DeepSeek 的 LLM API 进行认知处理。

## 构建与运行命令

```bash
# 构建（Windows 上使用 cargo.bat 以设置 MSVC 编译标志）
cargo.bat build              # debug 构建
cargo.bat build --release    # release 构建（LTO thin, strip, panic=abort）

# 运行服务器（监听 127.0.0.1:9110）
cargo.bat run
# 或
cargo.bat run --release

# 运行全部测试
cargo.bat test --lib

# 按名称运行单个测试
cargo.bat test --lib <test_name>

# Docker
docker-compose up --build
```

### 环境变量

- `TETRAMEM_API_KEY` — X-API-Key 请求头认证密钥（默认：`tetramem-dev-key`）
- `DEEPSEEK_API_KEY` — 认知/LLM 功能所需（分类、推理、别名生成）

### Windows 构建注意事项

请使用 `cargo.bat` 而非直接调用 `cargo`。它会设置 `CXXFLAGS=/MD` 和 `CFLAGS=/MD`，这是原生依赖（sqlite3、oniguruma）编译所需的。`.cargo/config.toml` 还设置了 `target-feature=-crt-static`。

## 架构

### 顶层模块 (`src/`)

| 模块 | 用途 |
|------|------|
| `main.rs` | Axum HTTP 服务器入口、路由定义、安全中间件 |
| `lib.rs` | 重新导出 `domain`、`engine`、`infra`、`api` |
| `infra.rs` | Space 快照的 JSON 序列化/反序列化、指标收集 |

### 领域层 (`src/domain/`)

核心几何原语，不包含业务逻辑或 I/O：

- **`vertex.rs`** — `Point3`（三维坐标）、`Vertex`（具名点）、`VERTEX_MERGE_EPSILON = 0.05`
- **`tetra.rs`** — `Tetrahedron`（4 个顶点，`EDGE_LENGTH = 1.0`）、`MemoryPayload`（内容、标签、嵌入向量、别名）
- **`space.rs`** — `Space`（空间索引）。使用 `RwLock` 保护的 HashMap 管理顶点、四面体、边/面表，以及基于网格的空间查找。当两个顶点距离小于 `VERTEX_MERGE_EPSILON` 时自动合并。通过 BFS 在共享顶点上检测簇。
- **`cylinder.rs`** — `Cylinder`（中央枢纽）、`Port`（端口）、`CylinderLayer`（本能/认知/服务/身份四层）。从底到顶堆叠。身份层无端口（确认后密封）。端口 ID 从 1,000,000 起始，避免与普通顶点 ID 冲突。
- **`channel.rs`** — 脉冲传播的通道跳转
- **`pulse.rs`** — 领域层脉冲类型定义

### 引擎层 (`src/engine/`)

业务逻辑子系统，通过 `Engine` 结构体统一连接：

- **`mod.rs`** — `Engine` 结构体（顶层编排器）。持有所有子系统的 `Arc` 引用并启动后台任务。
- **`bus.rs`** — `EventBus`（tokio broadcast 通道）。所有引擎中心通过 `EngineEvent` 枚举通信。
- **`energy.rs`** — `EnergyCenter` — 令牌桶速率限制器
- **`gateway.rs`** — `GatewayCenter` — 处理记忆创建：嵌入计算、分类、通过 `find_best_placement` 空间放置、知识图谱更新
- **`classifier.rs`** — `CategoryClassifier` — 基于 LLM 的分类（含降级方案）
- **`cognitive.rs`** — `CognitiveEngine` — DeepSeek API 客户端（支持工具调用）
- **`embedding.rs`** — `EmbeddingService` — 基于 HTTP 的嵌入降级方案
- **`vector.rs`** — `VectorLayer` — 进程内 ONNX 嵌入（主方案，从 `models/` 加载）
- **`scheduler.rs`** — `SchedulerCenter` — 周期任务运行器（自动脉冲、自动链接、去重、梦境循环）
- **`decision.rs`** — 自主操作决策逻辑
- **`pulse.rs`** — 引擎层脉冲机制（星型拓扑：端口 → 探索多面体 → 原路返回）
- **`knowledge.rs`** — `KnowledgeGraph` — 概念/关系管理
- **`reasoning.rs`** — 基于知识图谱的类比与模式推理
- **`dream.rs`** — 后台整理（去重、自动链接、衰减）
- **`emotion.rs`** — 情感状态管理
- **`dynamics.rs`** — 空间动态（簇演化追踪）
- **`radiation.rs`** — 记忆质量随时间的衰减/辐射
- **`identity.rs`** — 身份层管理器（首次运行确认流程，密封后锁定）
- **`constitution.rs`** — 系统宪法/规则执行
- **`security.rs`** — `SecurityGuard` — API 密钥认证、速率限制、验证、宪法检查
- **`storage.rs`** — `StorageManager` — 通过 `rusqlite` 的 SQLite 持久化
- **`mcp.rs`** — MCP 协议处理器
- **`hnsw.rs`** — HNSW 近似最近邻索引
- **`tools.rs`** — 认知引擎函数调用的工具注册表

### API 层 (`src/api/`)

- **`routes.rs`** — Axum 路由处理器。关键端点：`/remember`、`/ask`、`/search`、`/recall`、`/pulse`、`/identity`、`/dream`、`/knowledge`、`/reasoning/analogies`、`/stats`、`/sse`、`/health`
- **`middleware.rs`** — 请求中间件
- **`dashboard.html`** — 单页仪表盘 UI

### 数据流

1. 外部智能体发送 POST `/remember` 携带内容
2. 安全中间件验证 API 密钥 + 速率限制 + 能量
3. GatewayCenter 计算嵌入向量（ONNX 或 HTTP 降级）、分类内容、确定空间放置位置
4. 新四面体放入 Space — 找到最近的已有四面体，在其 `EDGE_LENGTH` 距离处放置，顶点在 epsilon 范围内自动合并
5. 簇拓扑从共享顶点自然形成
6. 调度器运行周期任务：自动脉冲（心跳）、自动链接、去重、梦境循环
7. 脉冲遵循星型拓扑：本能层端口 → 多面体簇 → 同一端口返回
8. 认知层每 5-15 tick 对脉冲报告运行 LLM 分析

### 并发模型

- 所有领域状态（`Space`、`Cylinder`、`KnowledgeGraph`）使用 `RwLock` 实现内部可变性
- 使用 `rlock!`/`wlock!` 宏（定义在 `space.rs` 中），通过 `unwrap_or_else(|e| e.into_inner())` 从中毒锁中恢复
- 引擎子系统通过异步 `broadcast::EventBus` 通信
- 后台任务通过 `tokio::task` 在 `Engine::start()` 中启动

## 关键设计常量

- `EDGE_LENGTH = 1.0` — 所有四面体的固定边长
- `VERTEX_MERGE_EPSILON = 0.05` — 顶点距离小于此值时合并
- 端口 ID 从 `1_000_000` 起始，避免与普通顶点 ID 冲突
- 圆柱体：`INITIAL_RADIUS = 2.0`、`INITIAL_HEIGHT = 8.0`、`PORTS_PER_RING = 8`

## 进行中的重构（源自 ARCHITECTURE_CONTEXT.md）

系统正从固定极坐标布局重构为纯物理空间模型。阶段 1（圆柱体）已完成。阶段 2-9 待实施：Space-Cylinder 集成、classifier/gateway/pulse 重写、身份流程、四层调度器、路由更新。依赖链为：Phase 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9，必须按顺序执行。

## 模型文件

`models/` 目录包含 ONNX 嵌入模型和分词器文件，供 `VectorLayer` 进行进程内嵌入计算使用。
