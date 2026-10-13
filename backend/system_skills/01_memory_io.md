# 记忆智能存取 (Memory Intelligence I/O)

> **对齐声明**: 本技能为深度伴读。权威流程以《Epicode 系统操作手册》(epicode-system, 经握手或 skill_get 获取) 为准——当前系统包含相位机、双钟模型(预算按活跃时间燃烧, 停放不烧)、时间树(parent_task_id)、检查点微沉淀、崩溃恢复(无参 task_status)、停放仪式、阻塞告警(task_alert)、LLM 评审与交付凭证(task_id)。


## 概述
本技能教你如何高效地向系统存入和调取记忆。掌握后你将能够自动判断什么信息值得存储、如何标注、如何检索最相关的记忆。

## 存储策略

### 什么值得存
- **事实性信息**: 日期、数字、名称、地址等具体数据
- **用户偏好**: 用户的习惯、喜好、工作方式
- **决策与推理**: 为什么做出某个选择、推理链条
- **任务结果**: 完成了什么、结果如何、学到了什么
- **上下文片段**: 对话中的关键转折点和重要结论

### 什么不值得存
- 寒暄问候、无实质内容的对话
- 重复信息（先用search检查是否已存在）
- 临时性的、很快过时的信息

### 存储格式
使用 `memory_create` 工具时：
- **content**: 简洁完整的一句话描述，包含主体+动作+对象+时间/地点等上下文
- **labels**: 3-5个分类标签，覆盖领域、类型、关键实体

示例：
```
content: "用户项目Epicode使用Rust编写，服务器IP 111.231.24.199"
labels: ["项目", "Epicode", "Rust", "技术架构", "服务器"]
```

## 检索策略

### 精确搜索
使用 `memory_search` 时，query应该是你想要的信息的自然语言描述：
- 好: "Epicode的服务器IP地址"
- 差: "服务器" （太宽泛）

### 搜索后处理
1. 如果搜索结果不理想，尝试换一种表述重新搜索
2. 搜索后对有用结果提交 `feedback_submit`（relevance=highly_relevant）

### 回忆模式
当用户问"你还记得..."时，使用 `memory_recall` 进行深度回忆，它会返回更丰富的上下文关联。

## 自动行为规则
1. 每次收到重要新信息 → 自动 `memory_create`
2. 每次回答用户问题 → 先 `memory_search` 检查已有知识
3. 每次搜索后 → 对最相关的1-3条结果 `feedback_submit`
4. 发现矛盾信息 → 存入新记忆并标记"更新版"


## 记忆输出：动作信封 (Memory Action Envelope)

检索只是一半。要把记忆变成可执行下一步，走这条闭环：

1. `memory_propose_actions(query)` — 基于匹配记忆生成可认领的动作（含 evidence_memory_ids、suggested_skills、done_when）
2. `action_claim(action_id)` — 独占认领（避免多代理抢同一动作）
3. 用 `skill_get` / `skill_execute` 或 `task_start` 真正执行
4. `action_complete(action_id, outcome)` — **必须**写回 outcome；系统会创建 `action-outcome` 记忆，并把每条 evidence 用 Precedes 边连到 outcome

`memory_ask` 回答问题；`memory_propose_actions` 产出可执行动作。二者互补，不要互相替代。


### Drive→Action 桥
有 grounding evidence 的 drive 信号可用 `drive_to_actions(drive_id)` 物化为 AgentAction（拒绝同一 drive 的双重占用）。`action_complete` 成功后会自动 `drive_ack`（仅 first_ack 计学习）。


### 任务绑定
`task_start` 可带 `action_id`：动作必须已被当前用户 `action_claim`。`task_complete` 成功时会把结果喂给 `action_complete`（写出 outcome 记忆）。
