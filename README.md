# Epicode — AI 记忆系统

为 AI 智能体与 LLM 应用提供**持久化记忆**的开源记忆系统：一个带认知生命周期的记忆云。

> 记忆不只是存储。Epicode 的记忆会做梦（自动整合）、有意志（自主信号环）、能生长（技能与经验沉淀）、可遗忘（时间效性与归档）。

## 核心特性

- **记忆引擎** — 四面体（tetrahedron）记忆结构 + 25 万级关系图 + HNSW 语义检索；存储层 AES-256-GCM 静态加密
- **双环认知架构** — 潜意识环（dream 整合、脉冲扩散、知识聚类）持续运行；主意识环（episodic focus、自我唤醒）按需点火
- **意志驱动系统** — 信号队列（warn/suggest/explore/constrain/request）、执行回执（drive_ack）、行为镜像
- **时间效性** — 记忆带 `valid_from / valid_to` 双时态字段；停止谈判、目标契约（done_when 证据映射）、饱和裁决
- **MCP 工具面** — 40+ 工具（memory_search / task_complete / kg_query / library_search...），任意 MCP 客户端即插即用
- **图书馆子系统** — 共享知识集合（批量 ingest、来源溯源、ACL 权限、请求-兑现工作流）
- **技能生态** — 语义自动触发、三层渐进披露、社区共享
- **多租户** — 每用户独立引擎与空间、API key 体系、套餐与限额、审计日志

## 三核心架构

```
        ┌─────────────────────────────────┐
        │         记忆基座 (本仓库)         │
        │  SQLite + HNSW + 加密存储 + 图书馆  │
        └──────┬──────────┬──────────┬────┘
               │          │          │
         ┌─────┴───┐ ┌────┴────┐ ┌───┴─────┐
         │ 语义核心 │ │ 直觉核心 │ │ 推理核心  │
         │ ~10ms   │ │ 33ms    │ │ 5-15s   │
         │ ONNX嵌入 │ │ 决策模型  │ │ LLM(可换)│
         └─────────┘ └─────────┘ └─────────┘
```

三核心共插同一记忆基座：语义让它可寻，直觉在历史判断上练成，推理结论回流为新记忆。

## 快速开始

```bash
# 构建 (Rust 1.75+)
cargo build --release --bin epicode-cloud

# 运行 (环境变量见下)
TETRAMEM_DATA_DIR=/path/to/data \
LLM_API_KEY=your-llm-key \
./target/release/epicode-cloud
```

| 环境变量 | 用途 | 默认 |
|---|---|---|
| `TETRAMEM_DATA_DIR` | 数据目录 | 必填 |
| `LLM_API_KEY` / `LLM_API_BASE` / `LLM_MODEL` | 推理核心（任意 OpenAI 兼容端点） | MiniMax |
| `EPICODE_PREWARM_PRIMARY` | 启动预热主引擎（内存紧张设 0） | 1 |
| `EPICODE_LAYA_PRESCREEN` | 直觉核心预筛评审 | 0 |

健康检查 `GET /health`，API 文档 `GET /openapi.yaml`，MCP 端点 `POST /mcp`。

## 文档

- [docs/](docs/) — 设计文档与运维手册
- Constitution（系统宪法）内置于 `src/engine/constitution.rs`

## 许可

[MIT](LICENSE) — 商用友好，欢迎共建。
