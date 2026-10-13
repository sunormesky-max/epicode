# 动作角色分工 (Planner ≠ Worker)

> 权威流程仍以握手 / `skill_get('epicode-system')` 为准。本技能说明 Memory Action Envelope 的角色面。

## Planner（规划面）
只产出方向，不独占执行：
- `memory_propose_actions` / `drive_to_actions` — 写入前沿
- `action_list` — 观察前沿

## Worker（执行面）
认领并闭环：
- `action_claim` — CAS 独占
- `skill_get` / `skill_execute` / `task_start(action_id)` — 真正干活
- `action_complete` — **必须**写 outcome（可被 `task_complete` 代劳）

## 规则
1. 不要在同一次回合里既大批量 propose 又假装已经完成 — 先 claim 再交付。
2. Viewer 角色默认只能 `action_list`（只读）；propose/claim/complete 需要写权限。
3. 同一 `source_drive_id` 在非终态时不可再次 `drive_to_actions`。


## ActionCard 输出契约
所有动作工具返回 `epicode.action_card`（schema_version=1）：id / summary / state / evidence_memory_ids / suggested_skills / done_when / stop_if / lineage。agent **不要自造字段名**；以 `cards` 或 `action` 对象为准。


## 调度分派与所有权 (Dispatch)
- `worker_register` / `worker_list` — 登记能力标签与负载上限
- `action_assign` / `action_dispatch` — 按能力匹配 + 最低负载分配；`assignee` 一旦写入不可改
- 仅 `assignee`（或 Admin）可 `action_complete`；外人 → 403
- `blocked`/`exhausted`：记责任人 → 再入队（清认领、提优先级、短期拉黑）或升级；禁止静默丢弃
