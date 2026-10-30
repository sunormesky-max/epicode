# 系统全览 (System Overview)

> **对齐声明**: 本技能只是导航页。完整的接入步骤和**全部工具清单**以《Epicode 执行者手册》(`skill_get("epicode-system")`，也是 `epicode_handshake` 返回的 `system_skill.content`) 为准；本页不再单独维护工具表，避免过期。

## 能力地图
| 领域 | 入口工具 | 深入阅读 |
|------|----------|----------|
| 接入 | `identity_step` → `identity_finalize` → `epicode_handshake` → `ctx_load` | 执行者手册 §1 |
| 记忆 | `memory_search` / `memory_recall` / `memory_create` / `memory_update` / `memory_forget` / `memory_delete` | 01 记忆读写 |
| 反馈与进化 | `feedback_submit` / `pattern_learn` / `dream_cycle` | 02 自动进化 |
| 技能 | `skills_sync` / `skill_get` / `skill_execute` | 03 技能发现 |
| 图谱 | `knowledge_relations` / `concepts` | 04 知识导航 |
| 任务 | `task_start` / `task_check` / `task_complete` | 10 详细参考 |
| 动作栈 | `memory_propose_actions` / `action_claim` / `action_complete` | 11 动作角色 |

## 快速接入流程
1. MCP `initialize`
2. 身份仪式 `identity_step`(1–5) + `identity_finalize`（已有身份则 `identity_confirm`）
3. `epicode_handshake(agent_id)`，**先读** `system_skill.content`（执行者手册）
4. `ctx_load(project, task)` + `enforced_rules(project)`，然后开始工作

## 核心原则
- **先搜后存**: 存新记忆前先搜索是否已存在
- **及时反馈**: 每次搜索后对结果提交 `feedback_submit`
- **标签丰富**: 每条记忆至少3个标签
- **主动存储**: 发现重要新信息时主动记住，不需要用户明确指示
