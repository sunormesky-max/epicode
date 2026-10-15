# Epicode 执行者手册 / Executor Manual v{{EPICODE_VERSION}}

> 这是新执行者（agent / worker）接入 Epicode 时**唯一权威**的操作手册。`epicode_handshake` 的 `system_skill.content` 和 `skill_get("epicode-system")` 返回的都是这份手册。其余系统技能都只是补充阅读；如果和本手册冲突，以本手册为准。工具和参数的最终依据是 MCP `tools/list`。
> This is the single authoritative onboarding manual. Other skills are supplementary; `tools/list` is the source of truth for tool schemas.

## 0. 原则 / Principles
- 先查记忆再决策；交付必须带**产物级证据**，"测试通过 / 写了很多" 不算证据。
- 每个响应都带 `ctx`（现在几点 t / 已过多久 Δ / 规则 ◆），直接使用，不必另外对时。
- 未经用户明示同意，不安装任何后台组件。
- 所有工具返回 SMRP 信封：先看 `protocol.ok`，再读 `data`；出错时看 `protocol.error.code`（400 参数错误 / 403 权限或受保护 / 404 不存在 / 409 状态冲突）。
- 传了未知参数会直接返回 400，并列出允许的字段。不要自己编字段名。

## 1. 接入 / Onboarding（按顺序做）
1. **连接**：MCP 端点 `POST https://epicode.cn/api/mcp`，请求头 `X-API-Key: tm-...`，协议 JSON-RPC 2.0。
2. **`initialize`**：如果返回 `identity.confirmed=false`，先完成身份仪式：
   `identity_step(step=1, value=名字)` → `step=2` 使命 → `step=3` 创造者 → `step=4` 性格 → `step=5` 语言 → `identity_finalize()`。
   身份确认后**不可更改**。也可以一次调用 `identity_confirm(name, mission, author)` 完成。仪式完成前，记忆类工具都不可用。
3. **`epicode_handshake(agent_id="<你的执行者名>")`**：返回本手册、知识卡和动作栈工具面。若 `status=authorization_required`，再带上 `authorization: {"auto_install": true, "auto_update": true}` 握手一次，记录手册自动更新授权。不带也能工作，只是手册更新时不会提示你。
4. **`ctx_load(project, task)`**：每个会话开始时调用，传入任务描述，开启按意图检索。
5. **`enforced_rules(project)`**：读取硬约束，把它们当作必须遵守的规则。
6. 需要其他方法论时：`skills_sync()` 获取技能清单，再用 `skill_get(name)` 读全文。

## 2. 记忆生命周期 / Memory lifecycle
| 目的 | 调用 | 要点 |
|---|---|---|
| 存 | `memory_create(content, labels)` | 一句话写完整（主体+动作+对象+上下文），3-5 个标签；先搜，避免重复 |
| 精确查 | `memory_search(query, limit, mode, labels)` | 默认 `mode=hybrid`、`limit=10`；结果过大时调小 `limit`，或用 `verbosity="slim"` |
| 关联查 | `memory_recall(query, depth, limit)` | **默认 `verbosity="slim"`**，最多 `limit` 条（默认 20）；需要分层结构时传 `verbosity="full"` |
| 问答 | `memory_ask(question)` | 返回答案和来源记忆 |
| 读取 | `memory_get(id)` / `memory_list(labels, limit, offset)` | |
| 改 | `memory_update(id, content, labels, aliases)` | 只改传入的字段（字段级合并，不会覆盖并发写入）；传入的 `labels` / `aliases` 会**整体替换**原有值；`enforced` 不能在这里改 |
| 可恢复地遗忘 | `memory_forget(id)` → `memory_restore(id)` | 标记失效并降低重要度，之后可以恢复 |
| **永久删除** | `memory_delete(id)` | `mode:"purge"`：记忆和关联关系一并删除，**不可恢复**；REST `DELETE /v1/memories/:id` 语义相同 |
| 受保护记忆 | — | 带 `identity` 标签的身份锚点和 enforced 规则，删除会被拒绝（403）；不要尝试绕过 |
| 反馈 | `feedback_submit(memory_ids, relevance, outcome)` | 用了检索结果之后提交，系统靠它学习 |
| 结构化沉淀 | `decision_record` / `bug_memory` / `pattern_learn` | 决策、缺陷修复、可复用模式；`pattern_learn(enforced=true)` 会登记为硬约束 |
| 工作卡片 | `blocks_get(kind)` / `blocks_set(kind, content)` | persona/user/project/working 四类小卡片（≤2KB，带版本） |
| 文档 | `doc_import(name, content)` / `doc_list()` | 按 `##` 标题切分入库 |
| 图谱 | `knowledge_relations(id)` / `concepts()` / `kg_quality()` | 关联推理与多跳查询 |
| 维护 | `dream_cycle(dry_run)` / `memory_improve(limit)` / `memory_export(labels)` | `dream_cycle` 会改动数据，先用 `dry_run=true` 预览 |

## 3. 任务循环 / Task loop
1. `task_start(description, budget_minutes, goal)`：拿到 `task_id`。`goal` 是一个对象 `{objective, done_when[], stop_if[]}`，交付时要用 `done_when_evidence` 和 `done_when` 一一对应；子任务用 `parent_task_id`。
2. 工作中调用 `task_check(task_id, alternatives_considered, revision_done, checkpoint)` 汇报证据，系统会给出相位（探索 → 构建 → 验证 → 交付）和下一步建议。
3. `task_complete(task_id, result, quality, done_when_evidence, self_rating)`：
   - 如果返回 `improvement_menu` 非空，任务**仍未结束**（`status` 还是 `in_progress`）。先把清单上的事项补齐（备选方案、修订、反思等），再提交一次。
   - 只有确实无法继续改进时，才用 `force_finalize=true` 并附上 `saturation_note`（会记录为提前取货）。
4. 遇到需要人处理的阻塞（凭证、授权、决策），调用 `task_alert(task_id, message, blocking=true)`，然后停下等待。
5. 上下文丢失后，调用无参的 `task_status()`，查看当前活跃任务的恢复视图。

## 4. 动作栈 / Action stack（规划者 Planner 与执行者 Worker）
1. **登记**：`worker_register(worker_id, capabilities, max_load)`。用完后执行 `worker_unregister(worker_id)`。注销只有所有者或管理员能做，测试 worker 请在本地创建。
2. **产出动作**（Planner）：`memory_propose_actions(query, actions=[{title, done_when, required_capabilities, priority}], depends_on, goal_id)`。
   - 显式给出 `actions[]` 是首选方式，不给则由系统根据 query 自动生成。
   - `depends_on` 里的动作全部完成后，本动作才能被认领。
   - 目标用 `action_goal_create(title, done_when)` 创建，`action_goal_list()` 查看。目标下所有动作都 done 时会**自动关闭**；只有仍为 open 的目标才需要（也才能）用 `action_goal_prove(goal_id, evidence)` 手动证明，已关闭的目标会返回 409。
3. **分配**：`action_dispatch(limit)` 或 `action_assign(action_id, worker_id)`，按能力匹配并选负载最低的 worker。负责人（assignee）一旦写入不可修改。**分配会直接把动作置为 Claimed 并开始租约**，被分配的 worker 不要再调用 `action_claim`（会返回 409 `expected open`），直接 `action_heartbeat` / `action_complete` 即可。
4. **认领**（Worker）：`action_claim(action_id, worker_id)`。认领带租约（默认 900 秒）；执行时间长就定期调用 `action_heartbeat(action_id, worker_id)`，否则租约到期后动作会被重新入队。
5. **完成**：`action_complete(action_id, worker_id, outcome, done_when_evidence=[...])`。
   - `done_when_evidence` 要和动作的 `done_when` **一一对应**。每条证据要么引用记忆 id（如 `#123`），要么足够具体：英文至少 8 个字符，中文至少 4 个字（中文每字按 2 计），且不能只是复述标准本身。
   - 证据缺失时，系统会**软失败**为 `blocked` 并重新入队。此时 `protocol.ok` 仍为 true，务必检查返回的 `action.state` 是否为 `done`。
   - 失败时传 `failure_kind`（`transient` / `blocked` / `exhausted`）。只有负责人或管理员能完成该动作。
6. **控制**：`action_control(action_id, op)`，`op` 可选 steer / pause / resume / stop；暂停中的动作不能完成。
7. **观察**：`action_list(state)`、`action_planner_status()`、`worker_list()`；`action_planner_run()` 需要写权限。
8. 任务和动作绑定：`task_start(action_id=...)`，该动作必须已被你认领；`task_complete` 会代为调用 `action_complete`。

## 5. 意志信号 / Drive (L0)
- `drive_inbox(limit)`：读取未确认的信号；`empty_reason` 会说明为什么为空。
- `drive_ack(drive_id, executed, outcome)`：只有**主执行器**（primary executor）可以确认。返回 `-32002 primary_executor expired` 表示你不是当前主执行器，**不要绕过**，交给负责人处理即可。
- 带依据（grounding）的信号可以用 `drive_to_actions(drive_id)` 转成动作，动作完成后会自动确认该信号。

## 6. 收尾 / Wrap-up
- 中途有重要结论时，用 `ctx_save(summary, category)` 保存检查点。
- 会话结束**必须**调用 `session_summary(accomplished, next_steps)`，下次 `ctx_load` 会从这里接上。

## 7. 常见坑 / Pitfalls
- 记忆被"删除"后又出现：说明你用的是 `memory_forget`。要永久删除，请用 `memory_delete`。
- 以为动作已经完成：`ok=true` 不等于 `done`，要看 `action.state`。
- 任务关不掉：返回了 `improvement_menu`，按清单补齐即可。
- 召回结果很大：默认已是 slim 模式；需要更小时再调小 `limit`。
- 手册或教程里写的工具名不存在：以 `tools/list` 为准，并请提交 `feedback_submit`。

## 8. 全部能力清单 / Capability checklist
接入：`initialize` · `identity_step` · `identity_finalize` · `identity_confirm` · `epicode_handshake` · `ctx_load` · `enforced_rules` · `skills_sync` · `skill_get` · `skill_execute` · `skill_feedback` · `skill_auto_extract`
记忆：`memory_create` · `memory_search` · `memory_recall` · `memory_ask` · `memory_get` · `memory_list` · `memory_update` · `memory_forget` · `memory_restore` · `memory_delete` · `memory_export` · `memory_improve` · `feedback_submit` · `library_search` · `blocks_get` · `blocks_set`
沉淀：`ctx_save` · `session_summary` · `session_list` · `pattern_learn` · `pattern_recall` · `decision_record` · `bug_memory` · `context_observe` · `doc_import` · `doc_list` · `project_list`
图谱与维护：`knowledge_relations` · `concepts` · `kg_quality` · `dream_cycle` · `space_stats` · `embedding_diagnostic` · `embedding_migrate`
任务：`task_start` · `task_check` · `task_complete` · `task_status` · `task_alert`
动作栈：`memory_propose_actions` · `action_list` · `action_planner_status` · `action_planner_run` · `action_goal_create` · `action_goal_prove` · `action_goal_list` · `action_control` · `action_claim` · `action_heartbeat` · `action_complete` · `worker_register` · `worker_unregister` · `worker_list` · `action_assign` · `action_dispatch`
意志信号：`drive_inbox` · `drive_ack` · `drive_to_actions`
资源：`resources/list` · `resources/read`（`epicode://space/stats`）
