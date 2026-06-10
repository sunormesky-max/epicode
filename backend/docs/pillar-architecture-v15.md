# TetraMem V15 — 通天柱架构设计文档

> 版本: v15.0-draft
> 日期: 2026-06-02
> 作者: 刘启航 (sunormesky-max)
> 状态: 设计阶段

---

## 1. 设计哲学

### 1.1 核心原则

中央调度器不是一个抽象的管理者，而是一个**实体几何对象**，存在于空间域中。

它的角色是：**记忆的摆渡人 (Ferryman of Memories)**。

- 所有记忆的创建、搜索、回收、合并都通过调度器表面的 Port 进行
- AI 智能体不直接访问空间中的四面体，而是通过摆渡人这个"港口"
- 摆渡人是空间域与外部世界之间的**唯一出入口**

### 1.2 通天柱 (Pillar of Heaven)

中央调度器的物理形态是一根**通天柱**——沿 z 轴延伸的圆柱体，贯穿所有用户的空间域。

- 多个用户的空间域沿 z 轴排列，共享同一根通天柱
- 这不是多实例，而是**单空间多域**
- z 轴不重叠 = 天然物理隔离
- 新用户 = 通天柱延伸一段

---

## 2. 当前架构的问题 (V14)

### 2.1 六个核心断裂

| # | 断裂 | 严重度 | 描述 |
|---|------|--------|------|
| 1 | Cylinder 层约束未施加 | 高 | find_best_placement 不按 CylinderLayer 约束 core.z |
| 2 | Port 系统从未连接 | 高 | assign_port/release_port 从未被 create_memory 调用 |
| 3 | Channel 未被 PulseEngine 使用 | 中 | PulseEngine 自建遍历，Channel 的 BFS 路径冗余 |
| 4 | 脉冲无层间门控 | 中 | 脉冲传播是平面的，不区分层内/跨层 |
| 5 | 聚类与放置度量脱节 | 中 | 放置用标签，聚类用 BFS 连通分量 |
| 6 | 三存储手动同步 | 高 | Space + HNSW + KG 删除时需手动协调 5 步 |

### 2.2 八个设计缺陷

| # | 缺陷 | 描述 |
|---|------|------|
| A | Cylinder 639行空转 | Port/expand/health_check 全部未被调用 |
| B | SchedulerCenter 上帝对象 | 25字段, 2282行, 承担 15+ 职责 |
| C | GatewayCenter 职责过重 | 9个依赖, 混合搜索+索引+分类+遥测 |
| D | 双调度循环重复 | quiet/full 两个循环大量重复逻辑 |
| E | EventBus 空转 | 8种事件中 5种无消费者 |
| F | 每tick全量快照 | build_snapshot() 全量 clone 所有数据 |
| G | Phase 编号跳跃 | 0→1→2→5→6, Phase 3/4 缺失 |
| H | 多用户=多实例 | N个用户=N份 Space/HNSW/KG/Engine |

---

## 3. 目标架构

### 3.1 物理结构总览

```
                 z 轴（向上延伸）
                 │
    ╔════════════╪═══════════════════════════════╗
    ║            │        通天柱 (Pillar)         ║
    ║            │          半径 R=2.0            ║
    ║            │                                ║
    ║   ┌────────┤────────┐ ← 用户N 的空间域      ║
    ║   │  z=[(N-1)H, NH) │                      ║
    ║   │                 │                      ║
    ║   │  ◆──◆──◆  N多面体  ← 共享顶点自然聚合  ║
    ║   │  │     │        │                      ║
    ║   │  ◆──◆  ◆        │                      ║
    ║   │     ↑            │                      ║
    ║   │  Port (共享顶点)  │                      ║
    ║   └────────┤────────┘                      ║
    ║            │                                ║
    ║   ┌────────┤────────┐ ← 用户1 的空间域      ║
    ║   │  z=[0, H)       │                      ║
    ║   │                 │                      ║
    ║   │  ◆──◆──◆        │                      ║
    ║   │  │     │  ◆     │  ← 孤立四面体        ║
    ║   │  ◆──◆  │        │                      ║
    ║   │     ↑  ↑        │                      ║
    ║   │   Port Port      │                      ║
    ║   └────────┤────────┘                      ║
    ║            │                                ║
    ║   ┌────────┤────────┐ ← 公共空间域          ║
    ║   │  z=[-H, 0)      │                      ║
    ║   │  公共Skills/知识 │                      ║
    ║   └────────┤────────┘                      ║
    ║            │                                ║
    ╚════════════╧═══════════════════════════════╝
```

### 3.2 每个用户空间域内部结构

```
    用户空间域 z=[base, base+H)  (H=用户域高度, 默认 8.0)

    ┌─────────────────────────────────────────────┐
    │  Identity 层  z=[base+6, base+8)             │  ← 身份记忆, 不可变
    │     Port Ring (8 ports, radius R)            │
    ├─────────────────────────────────────────────┤
    │  Service 层   z=[base+4, base+6)             │  ← 工具/技能记忆
    │     Port Ring (8 ports, radius R)            │
    ├─────────────────────────────────────────────┤
    │  Cognitive 层 z=[base+2, base+4)             │  ← 概念/推理记忆
    │     Port Ring (8 ports, radius R)            │
    ├─────────────────────────────────────────────┤
    │  Instinct 层  z=[base, base+2)               │  ← 原始/感知记忆
    │     Port Ring (8 ports, radius R)            │
    └─────────────────────────────────────────────┘

    通天柱表面在每个层内有 Port Ring
    Port 与该层内的四面体通过共享顶点连接
```

### 3.3 连接关系

```
    通天柱表面 Port ──共享顶点──→ 四面体A ──共享顶点──→ 四面体B
                                                    │
                                              共享顶点
                                                    │
                                              四面体C ──→ ... → N多面体

    通天柱 = 唯一出入口
    Port   = 触点（共享顶点坐标与四面体顶点重合）
    四面体 = 等边正四面体（EDGE_LENGTH=1.0）
    N多面体 = 多个四面体共享顶点自然聚合
    孤立四面体 = 仅连接到 Port，未与其他四面体共享顶点
```

---

## 4. 模块重构方案

### 4.1 Space → PillarSpace

当前 `Space` 是一个扁平的四面体容器。重构为 `PillarSpace`：

```rust
// 新增: 用户域描述
pub struct UserRealm {
    pub user_id: String,
    pub z_base: f64,
    pub z_height: f64,          // 默认 8.0
    pub next_layer_z: [f64; 4], // 每层内下一个可用 z 位置
}

pub struct PillarSpace {
    inner: RwLock<PillarInner>,
}

struct PillarInner {
    // 保留所有现有字段
    vertices: HashMap<VertexId, Vertex>,
    tetrahedrons: HashMap<TetraId, Tetrahedron>,
    edge_table: HashMap<(VertexId, VertexId), EdgeEntry>,
    face_table: HashMap<[VertexId; 3], FaceEntry>,
    vertex_to_tetras: HashMap<VertexId, Vec<TetraId>>,
    vertex_grid: HashMap<(i64, i64, i64), Vec<VertexId>>,

    // 新增
    pillar_radius: f64,                    // 通天柱半径 (2.0)
    realms: HashMap<String, UserRealm>,    // 用户域注册表
    public_realm: UserRealm,               // 公共空间域 (z < 0)
    next_z_base: f64,                      // 下一个用户域的 z 起点

    // Port 系统（从 Cylinder 移入）
    ports: Vec<Port>,                      // 通天柱表面所有 Port
    identity_store: HashMap<String, IdentityInfo>, // 用户身份
}
```

**关键变更**:
- Cylinder 的 Port 系统、LayerZone、expand() 全部融入 PillarSpace
- 每个用户的四面体被 z 轴区间约束
- 新用户注册 → `register_realm(user_id)` → 分配 z 区间 → 生成 Port Ring
- 公共空间在 z < 0 区域

### 4.2 Engine → 单实例摆渡人

当前每个用户一个 Engine 实例。重构为单实例：

```rust
// 旧: CloudState { users: HashMap<String, Arc<Engine>> }
// 新: CloudState { pillar: Arc<PillarEngine> }

pub struct PillarEngine {
    space: Arc<PillarSpace>,
    bus: Arc<EventBus>,
    ferryman: Arc<Ferryman>,        // 替代 SchedulerCenter
    gateway: Arc<GatewayCenter>,
    energy: Arc<EnergyCenter>,
    storage: Arc<StorageManager>,
    skills: Arc<SkillEngine>,
    cognitive: Arc<CognitiveEngine>,
    vector: Arc<VectorLayer>,       // 全局共享
}
```

**用户隔离方式**:
- 所有 API 调用传入 `user_id`
- `PillarSpace` 的所有操作接受 `user_id` 参数，内部做 z 轴过滤
- HNSW 搜索增加 `z_range` 过滤条件
- KG 关系增加 `owner` 字段

### 4.3 SchedulerCenter → Ferryman（摆渡人）

当前 SchedulerCenter 是 2282 行的上帝对象。拆分为：

```rust
pub struct Ferryman {
    // 核心
    space: Arc<PillarSpace>,
    cognitive: Arc<CognitiveEngine>,
    bus: Arc<EventBus>,

    // 子系统（各自独立 struct）
    observer: Observer,          // Phase 0: 观察（DriveEngine + 快照）
    executor: Executor,          // Phase 1-2: 执行（Pulse/Dream/Fission）
    feeler: Feeler,              // Phase 3: 情感（EmotionState）
    thinker: Thinker,            // Phase 4: 思考（LLM 决策）
    janitor: Janitor,            // Phase 5: 清洁（evict/save）
}

pub struct Observer {
    drive: DriveEngine,
    outcome: OutcomeTracker,
    adaptive: AdaptiveParams,
}

pub struct Executor {
    pulse: PulseEngine,
    dream: DreamEngine,
    fission: FissionEngine,  // 从 scheduler 中提取
}

pub struct Feeler {
    emotion: EmotionState,
}

pub struct Thinker {
    cognitive: Arc<CognitiveEngine>,
    security: Arc<SecurityGuard>,
    skills: Arc<SkillEngine>,
}

pub struct Janitor {
    storage: Arc<StorageManager>,
    last_save_tick: AtomicU64,
}
```

### 4.4 GatewayCenter → SearchService + IndexManager

当前 GatewayCenter 818 行，拆分为：

```rust
// 索引管理（写入时更新）
pub struct IndexManager {
    hnsw: Mutex<HnswIndex>,
    label_index: Mutex<HashMap<String, Vec<TetraId>>>,
    content_hash_index: Mutex<HashMap<u64, TetraId>>,
    dirty_set: Mutex<HashSet<TetraId>>,
    placement_cache: Mutex<HashMap<Vec<String>, Point3>>,
}

// 搜索服务（读取时调用）
pub struct SearchService {
    space: Arc<PillarSpace>,
    index: Arc<IndexManager>,
    vector: Arc<VectorLayer>,
    embedding: Arc<EmbeddingService>,
    cognitive: Arc<CognitiveEngine>,
    knowledge: Arc<KnowledgeGraph>,
}
```

### 4.5 EventBus → 真正的事件驱动

当前 EventBus 是日志通道。重构为消费者模式：

```rust
// 新增事件
pub enum EngineEvent {
    // 保留
    TetrahedronCreated { id: TetraId, owner: String },
    TetrahedronRemoved { id: TetraId, owner: String },
    TetrahedronMoved { id: TetraId, new_core: Point3, owner: String },

    // 新增
    MemoryStored { id: TetraId, owner: String, labels: Vec<String> },
    MemoryEvicted { id: TetraId, owner: String, reason: String },
    ClusterFormed { tetra_ids: Vec<TetraId>, owner: String },

    // 保留
    EnergyLow { remaining: f64 },
    Shutdown,
}

// 消费者注册
impl IndexManager {
    fn on_memory_stored(&self, id: TetraId, labels: &[String]) {
        // 自动更新 HNSW + label_index + content_hash_index
    }
    fn on_memory_evicted(&self, id: TetraId) {
        // 自动清理 HNSW + label_index + content_hash_index
    }
}

impl KnowledgeGraph {
    fn on_memory_stored(&self, id: TetraId, space: &PillarSpace) {
        // 自动 link
    }
    fn on_memory_evicted(&self, id: TetraId) {
        // 自动 remove_relations_for
    }
}
```

**效果**: 删除一个四面体只需 `space.remove_tetrahedron(id)` + `bus.publish(TetrahedronRemoved)` → 所有索引自动同步。

---

## 5. 物理层流转

### 5.1 记忆创建流程

```
智能体 → POST /v1/remember { content, labels }
  │
  ▼
cloud.rs → PillarEngine.gateway.create_memory(user_id, content, labels)
  │
  ├── 1. IndexManager 去重检查 (content_hash)
  ├── 2. EnergyCenter 能量消耗
  ├── 3. PillarSpace.realm_of(user_id) → 获取用户域
  ├── 4. CylinderLayer 分类 → 确定放置的层
  ├── 5. PillarSpace.find_free_port(user_id, layer) → 分配 Port
  │      Port 位置 = 通天柱表面在该层的一个点
  ├── 6. find_adjacent_position(port_position) → 四面体位置
  │      四面体的某个顶点与 Port 坐标重合 (vertex merge)
  ├── 7. PillarSpace.add_tetrahedron(tetra, positions)
  │      → 自动 vertex merge → 可能与已有四面体共享顶点
  ├── 8. EventBus.publish(MemoryStored)
  │      → IndexManager 自动更新 HNSW/label/hash
  │      → KnowledgeGraph 自动 link
  └── 9. 返回 TetraId
```

### 5.2 搜索流程

```
智能体 → POST /v1/search { query }
  │
  ▼
cloud.rs → PillarEngine.gateway.search(user_id, query)
  │
  ├── 1. VectorLayer.embed(query) → 查询向量
  ├── 2. PillarSpace.realm_of(user_id) → z_range
  ├── 3. IndexManager.hnsw_search(vector, z_range) → 候选集
  │      HNSW 搜索增加 z 轴过滤
  ├── 4. KnowledgeGraph 扩展（关系边遍历）
  └── 5. 排序返回
```

### 5.3 调度器 Tick 流程

```
每个 tick (120s):
  │
  ├── Phase 0: Observe
  │    Observer.observe()
  │      → DriveEngine 更新四维内驱力
  │      → 增量快照（脏标记，非全量 clone）
  │
  ├── Phase 1: Generate
  │    根据 drive 决定是否 pulse/fission/dream/evict
  │
  ├── Phase 2: Execute
  │    Executor.execute(actions)
  │      ├── auto_pulse()
  │      │   从 Port 出发 → 沿 Channel 传播
  │      │   层内: TTL=12, 温度低(精确)
  │      │   跨层: 需经过 Port 网关, TTL衰减大
  │      ├── auto_dream()
  │      │   merge≥0.95 → 合并
  │      │   evict junk → Port 释放
  │      └── auto_fission()
  │          高熵 N多面体 → 分裂
  │
  ├── Phase 3: Feel
  │    Feeler.update()
  │      → EmotionState PAD 分析
  │
  ├── Phase 4: Think (每 5 tick)
  │    Thinker.decide(user_id, state)
  │      → collect_state (仅用户的 z 区间)
  │      → CognitiveEngine.decide() → LLM
  │      → apply_thought() → 执行动作
  │
  └── Phase 5: Clean (每 10 tick)
       Janitor.save()
         → StorageManager 持久化
         → EventBus 自动同步索引
```

---

## 6. 数据结构变更

### 6.1 Tetrahedron

```rust
// 新增字段
pub struct Tetrahedron {
    pub id: TetraId,
    pub vertex_ids: [VertexId; 4],
    pub core: Point3,
    pub data: MemoryPayload,
    pub mass: f64,
    pub owner: String,        // 新增: 所属用户 ID
    pub pillar_port: Option<VertexId>,  // 新增: 连接的 Port ID
}
```

### 6.2 Relation (KnowledgeGraph)

```rust
pub struct Relation {
    pub source: TetraId,
    pub target: TetraId,
    pub relation_type: RelationType,
    pub strength: f64,
    pub owner: String,        // 新增: 所属用户 ID
}
```

### 6.3 Port (从 Cylinder 移入 PillarSpace)

```rust
pub struct Port {
    pub id: VertexId,
    pub position: Point3,
    pub layer: CylinderLayer,
    pub owner: String,        // 新增: 所属用户 ID
    pub connected_tetra: Option<TetraId>,
    pub status: PortStatus,
}
```

### 6.4 UserRealm (新增)

```rust
pub struct UserRealm {
    pub user_id: String,
    pub z_base: f64,
    pub z_height: f64,
    pub zones: [LayerZone; 4],
    pub port_count: usize,
}
```

---

## 7. API 变更

### 7.1 新增

| 端点 | 方法 | 描述 |
|------|------|------|
| `/v1/realm/info` | GET | 获取当前用户空间域信息 |
| `/admin/realms` | GET | 管理员查看所有用户域 |

### 7.2 行为变更

| 端点 | 变更 |
|------|------|
| `POST /v1/remember` | 自动按 CylinderLayer 分类放置 |
| `POST /v1/search` | 自动限定在用户的 z 区间 |
| `GET /v1/graph/analysis` | 仅分析用户的 z 区间 |
| `GET /v1/stats` | 仅统计用户的 z 区间 |

### 7.3 不变

所有现有端点保持兼容。user_id 通过 API Key 自动识别。

---

## 8. 实施计划

### Phase 0: EventBus 真正消费者 (预估 2-3 天)

**目标**: 解决三存储手动同步问题（断裂 #6）

**步骤**:
1. EngineEvent 增加 `owner` 字段
2. IndexManager 监听 `TetrahedronCreated/Removed` 自动更新索引
3. KnowledgeGraph 监听 `TetrahedronCreated/Removed` 自动 link/remove
4. 删除 scheduler.rs 中所有手动索引同步代码
5. cargo check 验证

**验证**: 删除一个四面体只需 `space.remove + bus.publish`，所有索引自动同步。

### Phase 1: Cylinder 接入放置 (预估 3-4 天)

**目标**: 四面体按 CylinderLayer 约束放置（断裂 #1, #2）

**步骤**:
1. Cylinder 激活: 分类记忆 → 确定 CylinderLayer
2. create_memory 流程: 分配 Port → Port 位置作为四面体某个顶点
3. find_best_placement 纳入 z 轴约束 (zone_for_layer)
4. Port 与四面体通过 vertex merge 物理连接
5. cargo check 验证

**验证**: 创建记忆时，四面体的某个顶点与通天柱表面的 Port 坐标重合。

### Phase 2: Channel 统一 (预估 2 天)

**目标**: PulseEngine 使用 Channel 替代自建遍历（断裂 #3）

**步骤**:
1. PulseEngine 的邻居遍历改为调用 Channel::find_channel
2. ChannelCache 融入 PillarSpace
3. 删除 PulseEngine 中的自建 snapshot + v2t 遍历
4. cargo check 验证

**验证**: 脉冲传播路径与 Channel BFS 路径一致。

### Phase 3: 脉冲层门控 (预估 2 天)

**目标**: 层内/跨层脉冲行为区分（断裂 #4）

**步骤**:
1. PulseEngine 增加层感知: 检查当前四面体所在 CylinderLayer
2. 层内脉冲: 高 TTL, 低温度, 精确传播
3. 跨层脉冲: 需经过 Port 网关, TTL 衰减大
4. cargo check 验证

**验证**: 脉冲优先在层内传播，跨层传播有额外衰减。

### Phase 4: SchedulerCenter 拆分 (预估 3-4 天)

**目标**: 从上帝对象到摆渡人核心（缺陷 B）

**步骤**:
1. 提取 Observer (DriveEngine + OutcomeTracker + AdaptiveParams)
2. 提取 Executor (PulseEngine + DreamEngine + FissionEngine)
3. 提取 Feeler (EmotionState)
4. 提取 Thinker (CognitiveEngine + SecurityGuard + Skills)
5. 提取 Janitor (StorageManager)
6. Ferryman 协调五个子系统
7. 合并双调度循环为单一循环
8. Phase 重编号为 0-5
9. cargo check 验证

**验证**: Ferryman 行为与旧 SchedulerCenter 完全一致。

### Phase 5: 单空间多域 (预估 5-7 天)

**目标**: 通天柱贯穿多个平行空间域（缺陷 H）

**步骤**:
1. PillarSpace 新增 UserRealm 注册表
2. 用户注册时分配 z 区间 + 生成 Port Ring
3. 所有操作接受 user_id 参数，内部 z 轴过滤
4. HNSW 搜索增加 z_range 过滤
5. CloudState 从 HashMap<Engine> 变为单 PillarEngine
6. 公共空间域 (z < 0) 放置公共 Skills
7. cargo check + 多用户集成测试

**验证**: 两个用户的记忆在物理上隔离（z 不重叠），但共享同一根通天柱。

### Phase 6: GatewayCenter 拆分 (预估 2-3 天)

**目标**: 搜索和索引职责分离（缺陷 C）

**步骤**:
1. 提取 IndexManager (HNSW + label_index + content_hash_index)
2. 提取 SearchService (search/ask/recall/remember)
3. GatewayCenter 保留能量控制和分类
4. cargo check 验证

**验证**: 行为不变，但代码职责清晰。

---

## 9. 风险与缓解

| 风险 | 缓解 |
|------|------|
| Space 单 RwLock 成为瓶颈 | 按 user_id 分段锁（每个用户域独立锁） |
| z 轴过滤增加搜索延迟 | HNSW 索引按 z 区间分区，减少候选集 |
| 重构期间服务器不可用 | ICP 备案期间进行，不影响线上 |
| LLM prompt 需要适配 | cognitive.rs 的 SYSTEM_PROMPT 增加 user_id 和 z_range 上下文 |
| 数据迁移 | 提供 migrate_v14_to_v15 脚本，为现有四面体分配 z 区间和 owner |

---

## 10. 与 V14 的兼容性

### 10.1 API 完全兼容

所有 `/v1/*` 端点保持不变。user_id 通过 API Key 自动识别，对智能体透明。

### 10.2 MCP 完全兼容

25 个 MCP 工具保持不变。内部实现改为通过 PillarEngine 路由。

### 10.3 数据迁移

提供 `migrate_v14_to_v15` 命令：
1. 读取每个用户的 SQLite 数据
2. 为每个用户分配 z 区间
3. 为每个四面体设置 owner 字段
4. 重新计算 Port 连接
5. 重建 HNSW 索引（含 z 过滤）

---

## 11. 预期收益

| 指标 | V14 (当前) | V15 (目标) |
|------|-----------|-----------|
| Cylinder 代码激活率 | ~20% (仅 identity) | 100% |
| Port 系统 | 0 个活跃 Port | 每用户 32+ Port |
| 空间拓扑 | 伪空间（无层次） | 真空间（z 轴分层） |
| 多用户模型 | N 个独立 Engine | 1 个 PillarEngine |
| 删除同步 | 手动 5 步 | EventBus 自动 |
| Scheduler 代码量 | 2282 行 | ~500 行 (Ferryman) |
| Gateway 代码量 | 818 行 | ~300 行 (SearchService) |
| 内存效率 | O(N * 用户数) | O(N) 共享 |
| 公共知识共享 | 不支持 | 通天柱公共空间域 |

---

## 附录 A: 术语表

| 术语 | 含义 |
|------|------|
| 通天柱 (Pillar) | 中央调度器的几何形态，沿 z 轴的圆柱体 |
| 摆渡人 (Ferryman) | 中央调度器的角色，记忆的唯一出入口 |
| Port | 通天柱表面的物理触点，与四面体共享顶点 |
| N多面体 | 多个四面体通过共享顶点自然聚合 |
| 用户域 (UserRealm) | 每个用户沿 z 轴占据的空间区间 |
| 公共空间域 | 通天柱的共享区域，存放公共 Skills 和知识 |
| 层 (CylinderLayer) | 用户域内部的 4 层结构 (Instinct/Cognitive/Service/Identity) |
| 拓扑通道 (Channel) | 通过共享顶点的 BFS 最短路径 |
| 脉冲 (Pulse) | 从 Port 出发的感知信号，沿拓扑传播 |

## 附录 B: 记忆参考

本次设计讨论产生的记忆 ID:
- #233: Graphify 算法研究（后期优化养分）
- #234: 记录研究成果的决策
- #235: V14 完整系统架构图
- #236: 架构设计问题审查（8 个问题）
- #237: 中央调度器作为摆渡人的设计哲学
- #238: 通天柱架构——中央调度器的终极形态
