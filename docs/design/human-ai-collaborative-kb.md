# Epicode 人机协同企业知识库设计

> 设计版本：v0.1
> 日期：2026-06-25
> 状态：概念设计阶段

---

## 1. 范式定义：从"AI 知识库"到"人机协同认知系统"

### 1.1 当前主流的三个层次

| 层次 | 特征 | 代表产品 | 局限 |
|------|------|---------|------|
| L1 检索增强 | 人搜文档，AI 从文档中找答案 | Notion AI、Glean | AI 是只读助手，不积累知识 |
| L2 辅助写作 | 人写，AI 补全/纠错/摘要 | Notion AI、Confluence智能 | AI 无记忆，每次从零开始 |
| L3 协同认知 | 人和 AI 共享记忆空间，协同思考 | （尚无成熟产品） | — |

**Epicode 的目标：L3。**

### 1.2 核心洞察

当前所有"AI 知识库"的根本问题是 **AI 没有自己的记忆**——每次交互都是无状态的，AI 只是一个带搜索的函数。

Epicode 已经有了空间记忆内核。如果让人和 AI **写入同一个记忆空间**，AI 就能：
- 记住"上次帮这个人写方案时，他改了什么"→ 下次主动避开
- 记住"这个团队反复问同一个问题"→ 主动整理成 FAQ
- 记住"这条知识已经 6 个月没人更新"→ 提醒验证
- 记住"AI 自己上次生成的摘要被人拒绝了"→ 调整生成策略

这就是**协同认知**：不是人用工具，是两个认知体共享一个大脑。

---

## 2. 共享记忆空间架构

### 2.1 记忆的 Provenance（来源标记）

每条记忆必须携带来源标记，这是信任的基础：

```rust
pub enum Provenance {
    /// 人类直接写入（文档/笔记/评论）
    Human {
        user_id: String,
        action: HumanAction, // Write / Comment / Annotate / Decide
    },
    /// AI 自动生成（摘要/链接/推理）
    AI {
        model: String,
        confidence: f64,
        trigger: AITrigger, // Summarize / Link / Infer / Discover
    },
    /// 人机协同产生（人写初稿 + AI 扩展，或 AI 生成 + 人修正）
    Co {
        human_user_id: String,
        ai_model: String,
        human_edits: u32,   // 人类修改次数
        ai_edits: u32,      // AI 修改次数
    },
    /// AI 检测到的人类知识冲突（需人工裁决）
    Conflict {
        conflicting_ids: Vec<String>,
        detected_by: String,
        detected_at: i64,
    },
}
```

### 2.2 信任分级

不同来源的记忆有不同的默认信任等级，影响检索排序和展示方式：

| Provenance | 默认信任 | 展示 | 检索权重 | 可被 AI 引用 |
|------------|---------|------|---------|-------------|
| Human (Write) | 最高 | 原文展示 | 1.0 | 直接引用 |
| Human (Comment) | 高 | 标注展示 | 0.9 | 直接引用 |
| Co (human_edits > ai_edits) | 高 | 标注协同 | 0.85 | 直接引用 |
| Co (ai_edits > human_edits) | 中 | 标注协同 | 0.7 | 标注"AI 主导" |
| AI (confidence > 0.8) | 中 | 折叠展示 | 0.6 | 标注"AI 生成" |
| AI (confidence < 0.8) | 低 | 默认隐藏 | 0.3 | 需人工确认 |
| Conflict | 最低 | 醒目标记 | 0.0 | 不可引用 |

信任等级可被用户手动覆盖（采纳/拒绝/提升/降级）。

---

## 3. 四种协同模式

### 模式一：AI 辅助写作（人主导）

```
人类写作 → 实时同步到记忆空间
         → AI 召回相关记忆 → 侧边栏展示"已有相关知识"
         → AI 检测"这段内容已有结论" → 标注
         → AI 检测"与已有知识矛盾" → 警告
         → 人采纳/忽略 → 反馈写入记忆
```

**关键技术**：
- 实时 streaming 检索（当前光标段 → embedding → recall）
- 增量写入（每段作为一个记忆片段，provenance=Co）
- 矛盾检测（新内容 vs 空间中近邻记忆的语义距离 + 关键事实对比）

**Epicode 现有能力复用**：
- `search` API 已支持语义检索
- `recall` API 已支持上下文召回
- 知识图谱已支持关系发现

**需新增**：
- WebSocket 实时推送通道
- 前端编辑器集成（类似 Notion AI 的 inline suggestion）
- 矛盾检测器（新模块）

### 模式二：AI 主动整理（AI 主导，人审核）

```
Dream Cycle 扩展：
  AI 扫描记忆空间 → 发现重复 → 提议合并
                   → 发现矛盾 → 创建 Conflict 记忆
                   → 发现主题聚类 → 生成主题摘要
                   → 发现孤立记忆 → 建议链接

  所有提议进入"待审核队列" → 人工批准/拒绝/修改
  审核结果反馈为 Co 记忆 → 训练 AI 的整理策略
```

**关键技术**：
- 扩展现有 `dream` 模块，从"静默整理"升级为"提议+审核"
- 审核队列（前端新页面）
- 批量操作 API

**Epicode 现有能力复用**：
- `dream` cycle 已有去重、链接、剪枝逻辑
- 知识图谱已有聚类能力
- `auto_pipeline` 已有后台任务调度

**需新增**：
- 提议系统（Proposal + Review 状态机）
- 审核队列 UI
- 策略学习（记录人的审核偏好，调整 AI 提议的激进程度）

### 模式三：协同问答（人提问，AI + 人协同答）

```
人提问 → AI 召回相关记忆（标 provenance）
       → AI 草拟答案（标注每句话的来源记忆）
       → 人补充/修正 → 补充部分写入为 Co 记忆
       → 答案沉淀为"已验证知识"（trust=最高）
       → 下次同类问题直接引用已验证答案
```

**关键技术**：
- 答案溯源（每句话链接到 source memory ID）
- 人的修正作为记忆写入（provenance=Co）
- 已验证答案缓存（避免重复回答）

**Epicode 现有能力复用**：
- `ask` API 已支持 LLM 问答
- `recall` 已支持上下文召回

**需新增**：
- 答案溯源标注
- 验证答案缓存层
- 问答线程管理（多轮对话）

### 模式四：AI 主动预警（AI 主动，人决策）

```
AI 定期扫描知识库健康度：
  → 检测知识衰减（某主题记忆长期无访问/更新）
  → 检测知识缺口（被问但无答案的问题）
  → 检测过时信息（时间敏感内容超期）
  → 检测知识孤岛（无入链的孤立记忆群）

  生成"知识库健康报告" → 推送给维护者
  维护者决策 → 更新/归档/补充 → 反馈写入记忆
```

**关键技术**：
- 知识衰减模型（基于访问频率 + 时间衰减 + 主题热度）
- 知识缺口检测（分析 search 返回 0 结果的查询日志）
- 健康度评分仪表盘

**Epicode 现有能力复用**：
- `stats` API 已有基础统计
- `timeline` API 已有时间线
- `energy` 模型已有记忆活力评估

**需新增**：
- 查询日志分析（记录 search/recall 的 0 结果查询）
- 衰减模型
- 健康度仪表盘
- 通知系统（邮件/IM/Webhook）

---

## 4. 信任与控制层

### 4.1 三层信任模型

```
┌─────────────────────────────────────────┐
│  层 3：组织策略层（Admin 配置）           │
│  - 哪些空间允许 AI 写入                  │
│  - AI 记忆的默认信任等级                 │
│  - 是否允许 AI 自动合并（无需人工审核）   │
│  - 数据保留与删除策略                    │
├─────────────────────────────────────────┤
│  层 2：空间配置层（Space Owner 配置）     │
│  - 本空间的 AI 行为模式                  │
│  - 审核流程（自动 / 需审核 / 禁止 AI）    │
│  - 信任阈值调整                          │
├─────────────────────────────────────────┤
│  层 1：个人控制层（每个用户）             │
│  - 查看每条记忆的 provenance             │
│  - 采纳/拒绝/修改 AI 记忆                │
│  - 个人信任偏好（总是折叠 AI 记忆等）     │
│  - "这个 AI 记忆帮我改一下"              │
└─────────────────────────────────────────┘
```

### 4.2 冲突解决机制

当 AI 检测到知识矛盾时：

1. **自动标记**：创建 `Conflict` 记忆，关联冲突的双方
2. **通知人类**：推送给相关空间的维护者
3. **冲突界面**：并排展示矛盾内容，人类选择/合并/标记"两者都对"
4. **解决记录**：解决过程本身作为记忆写入（provenance=Co），供 AI 学习

### 4.3 AI 记忆的生命周期

AI 记忆不是永生的，需要明确的过期和淘汰机制：

```
AI 记忆写入 → 进入"待确认"状态
            → 被人采纳 → 升级为 Co 记忆（信任提升）
            → 被人拒绝 → 标记"被拒绝"（信任降至最低，dream cycle 可清理）
            → 超时未处理（默认 30 天） → 自动降级 + 提醒
            → 多次被拒绝的同类提议 → AI 降低该类提议的生成频率
```

---

## 5. 数据模型扩展

### 5.1 记忆表扩展

在现有 `memories` 表基础上新增字段：

```sql
ALTER TABLE memories ADD COLUMN provenance TEXT DEFAULT 'human';
-- 'human' | 'ai' | 'co' | 'conflict'

ALTER TABLE memories ADD COLUMN provenance_meta TEXT;
-- JSON: {user_id, model, confidence, human_edits, ai_edits, ...}

ALTER TABLE memories ADD COLUMN trust_level REAL DEFAULT 1.0;
-- 0.0 ~ 1.0, 影响检索权重

ALTER TABLE memories ADD COLUMN review_status TEXT DEFAULT 'accepted';
-- 'pending' | 'accepted' | 'rejected' | 'expired'

ALTER TABLE memories ADD COLUMN parent_conflict_id TEXT;
-- 如果是冲突解决产生的记忆，关联冲突 ID

ALTER TABLE memories ADD COLUMN last_accessed_at INTEGER;
-- 用于衰减计算
```

### 5.2 新增表

```sql
-- AI 提议表
CREATE TABLE ai_proposals (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    proposal_type TEXT NOT NULL,  -- 'merge' | 'link' | 'summarize' | 'conflict' | 'archive'
    source_memory_ids TEXT NOT NULL, -- JSON array
    proposed_content TEXT,
    proposed_action TEXT,
    ai_model TEXT,
    confidence REAL,
    status TEXT DEFAULT 'pending', -- 'pending' | 'approved' | 'rejected' | 'modified'
    reviewer_id TEXT,
    reviewed_at INTEGER,
    review_feedback TEXT,          -- 人拒绝/修改的理由，用于策略学习
    created_at INTEGER NOT NULL
);

-- 查询日志（用于知识缺口检测）
CREATE TABLE query_logs (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    user_id TEXT,
    query TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    query_type TEXT NOT NULL,      -- 'search' | 'recall' | 'ask'
    created_at INTEGER NOT NULL
);

-- 知识健康度快照
CREATE TABLE knowledge_health (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    snapshot_date TEXT NOT NULL,
    total_memories INTEGER,
    human_ratio REAL,       -- 人类记忆占比
    ai_ratio REAL,          -- AI 记忆占比
    co_ratio REAL,          -- 协同记忆占比
    conflict_count INTEGER,
    avg_trust REAL,
    stale_count INTEGER,    -- 超期未更新
    orphan_count INTEGER,   -- 无入链
    gap_count INTEGER,      -- 知识缺口数
    health_score REAL,      -- 综合健康度 0-100
    created_at INTEGER NOT NULL
);
```

---

## 6. API 设计

### 6.1 新增端点

```
# 协同写作
POST /api/v1/collab/context     # 获取当前编辑上下文的相关记忆
POST /api/v1/collab/check       # 检查内容是否与已有知识矛盾
WS   /api/v1/collab/stream      # 实时协同通道

# AI 提议
GET  /api/v1/proposals          # 列出待审核提议
POST /api/v1/proposals/{id}/approve
POST /api/v1/proposals/{id}/reject
POST /api/v1/proposals/{id}/modify

# 知识健康
GET  /api/v1/health/space/{id}  # 空间健康度
GET  /api/v1/health/gaps        # 知识缺口列表
GET  /api/v1/health/stale       # 过时知识列表
POST /api/v1/health/scan        # 手动触发健康扫描

# 信任管理
POST /api/v1/memories/{id}/trust   # 调整信任等级
POST /api/v1/memories/{id}/adopt   # 采纳 AI 记忆
POST /api/v1/memories/{id}/reject  # 拒绝 AI 记忆

# 冲突
GET  /api/v1/conflicts           # 列出未解决冲突
POST /api/v1/conflicts/{id}/resolve
```

### 6.2 现有 API 扩展

```
# remember 扩展：支持 provenance
POST /api/v1/remember
{
  "content": "...",
  "provenance": "human|ai|co",    // 新增
  "provenance_meta": { ... },     // 新增
  "trust_level": 0.8              // 新增，可选
}

# search 扩展：支持 provenance 过滤
GET /api/v1/search?q=...&min_trust=0.5&provenance=human,co
```

---

## 7. 前端界面设计

### 7.1 记忆卡片增强

每条记忆卡片显示：
- **来源标签**：🟢 人类 / 🟣 AI / 🟡 协同 / 🔴 冲突
- **信任指示器**：5 星或百分比条
- **审核状态**：待确认 / 已采纳 / 已拒绝
- **操作按钮**：采纳 / 拒绝 / 修改 / 查看来源

### 7.2 新增页面

| 页面 | 功能 |
|------|------|
| 审核队列 | 列出所有 pending 的 AI 提议，支持批量操作 |
| 冲突中心 | 并排展示矛盾知识，支持解决 |
| 知识健康 | 仪表盘：健康度、衰减趋势、缺口、孤岛 |
| 协同编辑器 | 富文本编辑器 + AI 侧边栏（实时建议） |

### 7.3 AI 助手面板

在知识库的每个页面右侧常驻一个 AI 面板：
- **"这段已有结论"**：当用户浏览/编辑时，AI 实时召回相关记忆
- **"我发现了一个矛盾"**：主动提示冲突
- **"这个知识可能过时了"**：基于衰减模型提醒
- **"你可能想问"**：基于当前页面内容推荐相关问题

---

## 8. Epicode 独特能力的协同增强

### 8.1 空间记忆 → 协同空间

现有空间记忆模型按语义距离聚类。在协同模式下：
- **人类记忆**和**AI 记忆**在同一个空间中共存
- AI 可以"看到"人类记忆的聚类形状，推断"这个主题人类关注度高"
- 人类可以"看到"AI 记忆的空间分布，发现"AI 在这里生成了大量低信任记忆"

### 8.2 Dream Cycle → 协同整理

现有 dream cycle 是 AI 静默整理。升级为：
- dream cycle 的输出不再是直接修改，而是生成 **Proposal**
- Proposal 进入审核队列
- 人的审核结果反馈给 dream cycle，让它学习"这个团队喜欢什么样的整理方式"

### 8.3 Energy 模型 → 知识活力

现有 energy 模型追踪记忆的激活强度。扩展为：
- **人类 energy**：人工访问、引用、更新
- **AI energy**：AI 召回、引用、链接
- **协同 energy**：人在 AI 建议下访问/修改
- 能量极低的记忆 → AI 提议归档或更新

### 8.4 知识图谱 → 矛盾网络

现有知识图谱展示记忆间的关系。扩展为：
- 新增**矛盾边**（conflict edge）
- 新增**来源着色**（human 蓝色节点 / AI 紫色节点 / co 绿色节点）
- 可视化"知识信任地图"：高信任区域 vs 低信任待审区域

---

## 9. 实施优先级

### Phase 1：记忆溯源（2-3 周）

**目标**：让每条记忆可追溯来源

- [ ] 数据模型扩展（provenance, trust_level, review_status）
- [ ] `remember` API 支持 provenance 参数
- [ ] `search` API 支持 provenance 过滤
- [ ] 前端记忆卡片显示来源标签和信任指示器
- [ ] 手动采纳/拒绝 AI 记忆的 API 和 UI

### Phase 2：AI 提议系统（3-4 周）

**目标**：让 dream cycle 从静默变可见

- [ ] `ai_proposals` 表和 Proposal 状态机
- [ ] 扩展 dream cycle 生成 Proposal 而非直接修改
- [ ] 审核队列 UI
- [ ] 批量审核 API
- [ ] 审核反馈记录（用于策略学习）

### Phase 3：矛盾检测与冲突解决（2-3 周）

**目标**：让 AI 能发现知识矛盾并引导人类解决

- [ ] 矛盾检测算法（语义距离 + 事实对比）
- [ ] `Conflict` 记忆类型
- [ ] 冲突中心 UI（并排展示 + 解决操作）
- [ ] 知识图谱矛盾边可视化

### Phase 4：知识健康度（2-3 周）

**目标**：让知识库"自感知"健康状况

- [ ] 查询日志记录
- [ ] 知识衰减模型
- [ ] 知识缺口检测
- [ ] 孤岛检测
- [ ] 健康度仪表盘
- [ ] 通知系统

### Phase 5：协同编辑器（4-6 周）

**目标**：让写作过程实时协同

- [ ] WebSocket 实时通道
- [ ] 编辑器上下文召回
- [ ] 实时矛盾检测
- [ ] AI 侧边栏建议面板
- [ ] 协同记忆写入（provenance=Co）

---

## 10. 与竞品的差异化定位

| 能力 | Notion AI | Glean | Confluence | **Epicode** |
|------|-----------|-------|-----------|-------------|
| AI 有自己的记忆 | 无 | 无 | 无 | **有（空间记忆）** |
| AI 主动整理知识 | 无 | 无 | 无 | **有（dream cycle）** |
| 人可审核 AI 的修改 | N/A | N/A | N/A | **有（Proposal 系统）** |
| 知识矛盾检测 | 无 | 无 | 无 | **有（Conflict 检测）** |
| 知识健康度监控 | 无 | 无 | 无 | **有（衰减+缺口+孤岛）** |
| AI 记忆可追溯 | N/A | N/A | N/A | **有（provenance）** |
| 知识图谱可视化 | 无 | 无 | 有（基础） | **有（空间+图谱+矛盾边）** |
| MCP 工具集成 | 无 | 无 | 无 | **有（35 个 MCP 工具）** |

**核心叙事**：Epicode 不是"带 AI 的知识库"，而是"人和 AI 共享大脑的知识系统"。

---

## 11. 风险与挑战

| 风险 | 影响 | 缓解 |
|------|------|------|
| AI 记忆质量低导致信任崩塌 | 用户关闭所有 AI 功能 | 信任分级 + 默认折叠 + 人工审核 |
| 冲突过多淹没用户 | 审核队列积压 | 冲突优先级排序 + 批量操作 |
| 策略学习需要大量数据 | 冷启动困难 | 预置合理默认策略 + 逐步学习 |
| 实时协同的延迟 | 写作体验差 | 增量检索 + 本地缓存 + 后台同步 |
| 隐私顾虑（AI 读取所有知识） | 企业不敢用 | 空间级 AI 开关 + 加密 + 审计日志 |

---

## 12. 总结

Epicode 向企业知识库演进的核心差异化不是"再做一个 Notion"，而是**利用已有的空间记忆内核，构建人机共享的认知空间**。

传统知识库的根本问题是知识只增不减、越来越乱、没有人整理。Epicode 的 dream cycle + 协同整理 + 健康度监控天然解决这个问题——AI 充当永不疲倦的知识管家，人类做最终决策。

这把 AI 从"问答工具"升级为"认知伙伴"，是企业知识管理领域目前没有人做到的定位。
