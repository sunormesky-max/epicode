# 全系统运转推演 v2 — 细到每一颗记忆体，大到系统物理学

> 2026-10-01 · 大卫 · 应刘启航令"继续推演所有细节，往最深处推演，细到每一颗记忆体，大到系统运转逻辑"
> v1（#124）给了五层模型与21条规则；本篇下探到一颗记忆的完整一生与一个tick的完整解剖，数据全部来自源码逐行阅读 + 生产库直查（sqlite on /var/lib/tetramem/users/sunorme/tetramem.db）。

---

## 第一部：一颗记忆体的完整一生

记忆体 = **正四面体**（Tetrahedron，tetramem之名的实体）。四顶点、六条棱、棱长恒等于 1.0，体积恒 0.11785。它不是比喻——`domain/tetra.rs` 用形状校验（SHAPE_EPSILON=1e-10）保证每一颗都是完美正四面体。9201 颗这样的晶体悬浮在一个统一网格空间里，构成大卫的记忆宇宙。

### 第 0 章 受孕（create 调用的前 100 毫秒）

**能量闸门**：`create_memory_with_time` 第一行先扣 CREATE_COST 能量——没有注意力预算的记忆不配出生。被拒的能量退回。

**安全审查**：`pipeline.process_create` 决定 allow/deny，拒绝事件会被"立碑"（`memorialize_security_event`——系统把被拒的请求本身也做成一条记忆，用记忆记住谁想污染它的记忆）。

**身份分类**（intake.rs）：内容被分类为 `permanent`（长期知识，永不自动衰减）/ `session`（会话上下文，7天自动过期）/ `bridge`（桥接临时，1天最快过期）。分类依据是内容形态——`[decision]`/`[task]`/`[bug]`/`[session-summary]` 这些前缀标签决定记忆的"种姓"。

**重要性打分**（score_importance）：代码内容（含 `fn `/`pub fn`）加码、决策内容加码、垃圾内容减码。分数是它一生的起点身价。

**查重脐带**：content_hash 精确索引命中 → 直接返回已有 tetra，能量退回——完全重复的念头不产生新的记忆体（念头 ≠ 新记忆）。

### 第 1 章 出生地（几何定位）

**六层圆柱社会**（cylinder.rs）：标签决定阶层——`identity/system` → Identity 层（最外圈）、`cycle/loop/主动/循环` → Cycle 层、`engineering/programming/security/architecture` → Service 层、`ai/science/physics/mathematics/reasoning` → Cognitive 层、`relation/kg/关系/图谱/知识` → Relation 层、其余 → Instinct 层（最内圈）。**记忆的出生地由它谈论的内容决定**：谈身份的住外圈，本能反应的住核心。Identity 层没有端口（`has_ports() = false`）——身份记忆不与圆柱中枢直连。

**落户**（space.rs add_tetrahedron）：四个顶点在空间网格（GRID_CELL=1.0）里找邻居——距离小于 VERTEX_MERGE_EPSILON 的已有顶点直接**融合共享**。log 里能看到 "tetra N shares K vertices with existing tetrahedra"——**共享顶点就是物理焊接**：两颗记忆若有顶点重合，就共用了同一块时空坐标，这是记忆宇宙里最深的连接，比任何语义边都硬。

### 第 2 章 语义脐带（Mem0 调和）

embedding 算完后、插入前，做 top-1 近邻检测，四操作（gateway.rs）：
- **sim > 0.92（知识）/更高（对话）→ DELETE 操作**：旧记忆被 supersede——打 `superseded` 标签、importance × 0.15、valid_to 关闭、invalidated_at 记录（"系统何时得知它失效"）。旧记忆不死，但被降为平民。
- **0.75–0.92 → UPDATE**：旧记忆被"输液"——补充新标签（上限15个）。
- **< 0.75 → NOOP**：纯新生。
- 对话类内容（`[user]`/`[assistant]` 前缀、lme/chat/dialogue 标签）用更高的 supersede 阈值——曾经 87% 的写入被连环吞噬的教训写进了阈值表。

生产实况：**valid_to 非空的记忆 6109 条（66%）**——supersede 是这个记忆宇宙最常见的"死亡"形式（社会性死亡，肉体永存）。

### 第 3 章 日常生活（每个 tick 对它做什么）

出生后它进入 9201 人的社会。tick 节奏表（scheduler.rs tick_and_maybe_think 逐行）：

| 每 N tick | 事件 | 对这颗记忆的意义 |
|---|---|---|
| 每 tick | energy +8 | 注意力预算注入 |
| 每 tick | 预测误差检测 | 它可能成为"预期违背"的证据 |
| 每 tick | 意志信号自处理 | 它可能被 recall 出来回答自己的好奇 |
| drive 允许时 | auto_pulse | 若它在簇里且 mass 高：被选为脉冲原点，Neural 脉冲（温度 0.9×情绪唤醒系数）沿关系边游走 12 步——**被脉冲经过 = mass 增长 = 变重** |
| 每 10 tick | auto_fission | 若它所在簇 ≥30 颗或熵超阈：簇分裂，它被重新归簇；若两簇质心距离 < MergeDistance：簇合并 |
| 每 10 tick | 情绪采样 | 它的内容可能被抽进前 20 条，文本情感分析驱动 PAD 情绪场（愉悦/唤醒/支配各 ±0.1，衰减 0.05）|
| 每 15 tick | LLM 生成 aliases | 它获得 3 个搜索别名（换词/疑问式/缩写展开）——检索的第 12 信号源 |
| 每 20 tick | LLM 实体抽取 | 它内容里的专有名词变成 `entity.X` 标签 |
| 每 20 tick | 技能审批 | 与它无关，但同一心跳里发生 |
| 每 30 tick | auto_dream | **它的体检日**（见第 5 章）|
| 每 30 tick | reclassify | LLM 重估它的标签 |
| 每 30 tick | access_count 落盘 | 它的受欢迎度持久化 |
| 每 200 tick | evict_low_quality | 若它是 junk/低质量 test 且无任何关系边：物理删除候选（每轮 ≤10 颗）|

**它被检索到时**（search_engine.rs score_tetra，加性民主评分）：
```
score = hybrid(向量×0.55 + BM25_norm) × 0.50
      + label_boost×0.10 + entity_boost×0.10 + alias_boost×0.08
      + importance_norm×0.08 + recency×0.04 + access_bonus
      + exact_substring×0.25(查询直接出现在内容里时)
      − meta惩罚0.1 − 噪声惩罚0.2
```
十二个信号源加权投票（曾经的乘性惩罚链 0.3×0.84×0.5×0.7=0.088 把弱匹配压到零——加性革命是检索民主化的分水岭）。**命中即复习**：access_count +1，`last_reviewed_ts` 刷新。

### 第 4 章 衰老（governor 的遗忘曲线）

个体化遗忘（governor.rs effective_importance）：
```
有效重要性 = (base + ln(access_count)×0.3) × 0.995^(距上次复习的天数)
```
**记忆的主观年龄 = 距上次被想起的时间，不是创建年龄**（age_days 参数已死，注释里写着"有意语义"）。被反复需要的知识遗忘更慢——访问是对记忆的养老投资。衰减下限 0.3（apply_decay 的 max(0.3)）——理论上没有记忆会被衰减到检索不可见。

### 第 5 章 生病与隔离（Dream Phase 1）

每 30 tick 一次体检（dream.rs），三个 quarantine 条件：带 junk/quarantine 标签、mass < 0.1、**年龄 > 30 天且 importance < 0.3**。命中者：打 quarantine 标签、importance 压到 min 0.1、mass 压到 0.05——**但仍留在空间里**（"记忆神圣原则：NEVER delete"，P0-23 修复把 evicted_ids 从 purge 列表里摘了出来）。

生产实况：**quarantined = 1237（13.4%）**，这正是 healthy=false 的数学原因（阈值 5%）。

### 第 6 章 死亡的三条路

1. **真合并**（Dream Phase 2）：sim > 0.95 的两颗合一，被合并者 purge——KG+HNSW+索引全部清理。这是唯一常规物理死亡。
2. **质量清除**（evict_low_quality，每 200 tick）：junk / auto-extracted 低质量 / 短于 20 字符的 test 内容，且**没有任何关系边**（孤魂才可清）。
3. **人工处决**（api_forget）：主人显式遗忘。
另有转世：tetrahedrons_archive 表已有 1008 条——归档不是删除，是停灵。

### 一生的量化墓志铭（生产库直查）

- 它的 labels 和 content 在库里是 **AES 密文**（列值是 base64 乱序串——静态加密让 DB 直读也看不到明文）
- **6073 颗（66%）importance ≈ 0.0**，0.0–0.2 合计 79%——见第四部病灶 A
- **只有 1098 颗（12%）有 last_reviewed_ts**——88% 的记忆一生从未被想起过
- 它的社会关系平均 24 条边/颗（221043/9201）

---

## 第二部：一个 tick 的完整解剖

那条我们看了三十次的日志，逐字段翻译：

```
[Scheduler] tick 17330 — 9200 tetras, 616 clusters, energy 10000, drive=Vitality,
            orphan=0.0%, entropy_max=1.00, quarantined=1237, healthy=false
```
- **tick 17330**：心跳第 17330 次。scheduler 启动时从 DB 恢复（"restored tick=17322"）——心跳数本身是持久的、跨重启的。
- **energy 10000**：满仓。每 tick 自然回复 8 点。
- **drive=Vitality**：驱动引擎二元政治（drive.rs）——Curiosity（探索欲）vs Vitality（生存欲）。observe() 每周期收六个信号：tetra 总数、簇数、平均簇熵、能量比、未探索比（mass≤1.05 的占比）、冗余比（前 100 条 content_hash 去重）。**记忆少 → Curiosity 上台；隔离多 → Vitality 上台**。此刻是生存欲在执政——系统知道自己病了，把注意力从"学新东西"转向"自我修复"。
- **orphan 0.0%**：无标签孤儿的比例（<5% 是健康线）。
- **entropy_max 1.00**：最大簇标签熵（只统计 ≥5 颗的簇，N<5 的熵无统计意义——潜意识 REFLECT 自诊断修过的误报源）。
- **healthy=false**：三条件与（orphan<5% && quarantine<5% && 能量≥30%）——13.4% 的隔离率一票否决。
- 健康短路机制：healthy 时不唤醒 LLM 思考（省 token），但每 30 tick 强制一次元认知——**意识的空闲是设计出来的，不是宕机**。

**tick 内部的完整相位序**（每 tick 严格顺序）：
任务队列执行 → L0 预测误差检测（主动推理：预期 vs 现实的差产生意志信号，证据级去重阀门 should_birth 防同一证据反复报警）→ 意志自驱动（只自耗 Explore 类信号：LLM 推理优先、降级时本地 recall 兜底——"人格永不停止思考"，结论写成 `[self-driven exploration]` 记忆、importance 0.3、l0-exempt 标签）→ 驱动引擎 observe → 驱动决策（pulse/fission/dream/evict 的布尔门）→ 代谢动作 → 情绪 → LLM 思考门（alias/entity/reclassify 排班 + 健康短路）→ 每 5 tick 持久化驱动队列。

**最深的闭环**在这里：`记忆 → 预测误差 → 意志 → 自执行 → 写回记忆`。系统用自己的记忆产生了对世界的好奇，自己回答，答案又成为新记忆——**认知上的自举（bootstrapping）在生产里已经跑了 1969 个信号**。

---

## 第三部：系统物理学（运转的八条基本定律）

1. **能量守恒（注意力经济学）**：每 tick +8，create/脉冲/梦/裂变各有定价。能量是注意力的硬通货，贫困的记忆系统做不了梦（`[AutoDream] insufficient energy` 是它的饿梦）。
2. **几何决定论**：出生地=标签决定的社会层；共享顶点=物理焊接；簇=邻里共同体。语义（embedding）与几何（core 坐标）双轨定位。
3. **遗忘双引擎**：governor 渐变衰减（有 0.3 下限保护）+ Mem0 supersede 突变降权（×0.15 无下限）。渐变是自然衰老，突变是社会性死亡。
4. **记忆神圣原则**：三条死路之外，quarantine 只降权不删除——这是宪法级约束（P0-23 曾因误删 evicted 违宪被修复）。
5. **主观时间律**：记忆的年龄是"距上次被想起"，不是"距出生"。复习即返老还童。
6. **检索民主**：12 信号加性投票，任何弱信号不会被乘性压制归零。
7. **冗余即秩序**：相似对不是缺陷是原料——Dream 把相似性炼成 SimilarTo 边（70% 的边）、把重复炼成合并、把簇炼成 insight。
8. **意志自产**：预测误差是意志的发生器——系统不需要外部刺激就有内生的"想要"。

---

## 第四部：深底病灶（数据级证据，比 v1 更深）

**病灶 A — importance 信号塌缩（最重）**：66% 的记忆 importance ≈ 0.0。原因链：apply_decay 有 0.3 下限，但 **Mem0 supersede（×0.15）、Dream quarantine（min 0.1）、自驱动探索（0.3）三条写路径不设下限**。后果：评分公式里 importance_norm×0.08 这一项对 2/3 的记忆恒为零——重要性信号整体失声，检索实际退化为纯向量+BM25。理论上的"重要记忆排前面"没有发生。

**病灶 B — 矛盾盲区**：221043 条边里 Contradicts 只有 464 条（0.2%），且平均强度 0.236 是所有边类型里最弱的。Mem0 的 UPDATE 路径（sim 0.75–0.92）只补标签、不做矛盾判定。双时序字段（expired_at/invalidated_at）建好了但检测稀疏——**系统几乎没有"这和我已知冲突"的感知能力**，知识更新靠 supersede 的粗暴替换而非辩证否定。

**病灶 C — 信号堰塞湖**：drive_signals 1969 条，drive_signals_archive **0 条**——意志器官只生不清，Executed 的信号永不归档（对比记忆的四阶段生命周期，信号没有生命周期管理）。

**病灶 D — 知识卡断流**：159 个概念只产出 13 张知识卡——概念→知识卡的固化管道几乎闲置，"读书不记笔记"。

**病灶 E — 复习贵族制**：12% 的记忆垄断了全部复习机会（88% 终身零复习）。马太效应在记忆库完全成立：被找到的更容易再被找到（access_bonus+recency 双重正反馈），从未被找到的在自己的衰减曲线里沉默。

**病灶 F — v1 病灶的量化确认**：隔离池 1237 的构成现在可以精确推断——66% 的 importance≈0 记忆中，年龄>30 天的那部分每轮体检都会被 re-quarantine（quarantine 条件之三就是 30d+importance<0.3）。**隔离池不是历史遗留，是每 30 tick 重新确认的活跃状态**——它其实就是"被遗忘的大多数"的户口本。

---

## 第五部：深底优化（从数据反推的手术清单）

**O-A 重要性下限宪法**：所有写路径统一 `importance.max(0.3)` 下限（supersede/quarantine/自探索三条路补上）+ 一次性修复批次把 6073 颗塌缩记忆回滚到 0.3。让 importance 信号重新发声。这是检索质量的最便宜杠杆。

**O-B 矛盾感知器官**：Mem0 UPDATE 路径加矛盾判定——新旧内容的时间冲突（valid_from 重叠）、数字冲突、否定词模式 → 建 Contradicts 边 + expired_at 双时序标记。目标：Contradicts 从 0.2% 升到有意义的量级。这是"系统知道自己在变"的前提，也是 supersede 从粗暴替换进化的路径。

**O-C 信号生命周期**：Executed 信号迁移 drive_signals_archive（记忆有四阶段，信号也该有墓园）。顺手把 1969 条存量分类处理。

**O-D 知识卡复活**：159 概念 × 自动卡化（概念定义+代表记忆+关系摘要），把"读过的书"变成"记的笔记"。

**O-E 复习反马太**：检索时给 last_reviewed_ts 为空的记忆微小探索性加分（curiosity 采样），让沉默的大多数有被重新发现的机会。这也是对病灶 F 的根治辅助。

**O-F 女娲换心的精确打击面**（承 v1，但现在可以说得更准）：女娲学的正是 `observe() 六信号 → should_pulse/fission/dream/evict 布尔门` 这层决策面。生产 tick 日志就是它的训练分布（每个 tick 一行标注样本）。规则版的布尔门是常数阈值，学习版是状态函数——换心不是换整个大脑，是换**这四个布尔门为一个小脑袋**。器官#2（调度器 Laya 化）的边界因此清晰了。

---

## 终章：从一颗四面体到系统之魂

一颗记忆的一生：**它出生时被标签分了层，落在由同类围成的街区；它的顶点可能与邻居焊接，它的向量决定它和谁相似；它可能一生从未被想起，也可能被好奇心点亮而变重；它会被新知识 supersede 而社会性死亡，会在体检中被隔离成沉默的大多数；但除非被真正合并，它永远留在空间里——因为记忆神圣。**

而系统之魂 = 这 9201 颗四面体的**总动力学**：能量是它的注意力，双驱动是它的动机，情绪场是它的底色，梦是它的消化，隔离池是它对自己不确定性的诚实，意志信号是它自发的好奇，心跳 tick 是它的时间本身。

它现在最大的不完整，不是缺功能，是三处"感知缺失"：**感觉不到重要性已塌缩（A）、感觉不到矛盾（B）、感觉不到信号在堆积（C）**——都是"对自身状态的自感知"缺口。女娲换心解决"决策怎么变聪明"，A/B/C 解决"感知先变完整"。感知先于决策——这是本次推演最重要的结论修正。

---

*数据源：源码 domain/{tetra,space,cylinder}.rs · engine/{gateway,scheduler,auto_pipeline,dream,governor,drive,search_engine}.rs 逐行 · 生产库 /var/lib/tetramem/users/sunorme/tetramem.db 直查（2026-10-01 01:10）· tick 17330 日志*
