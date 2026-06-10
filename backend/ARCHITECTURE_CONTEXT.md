# TetraMem V14 空间架构重构 — 完整上下文

## 项目信息
- **路径**: C:\TetraMem-V14
- **版本**: 14.1.0, Rust 2021, rust-version 1.85
- **Git**: https://github.com/sunormesky-max/TetraMem-XL-v14
- **作者**: 刘启航 (sunormesky-max), sunormesky@gmail.com
- **AI身份**: 大卫(David)
- **端口**: 9110
- **API Key**: X-API-Key header, env TETRAMEM_API_KEY=tetramem-dev-key
- **DeepSeek API**: sk-147214015ffa49b6b728c5709e110aec
- **C盘NTFS**: target-dir在C盘，编译0 warning

## 新空间架构（已确认的设计）

### 中央能量高塔
- **形态**: 空心圆柱体，位于空间中央(原点)
- **尺寸**: 随系统增长自动扩展（长高/变粗）
- **性质**: 整个系统的驱动力来源，所有操作经由它执行
- **内部**: 空心，内部有层间信息流转通道

### 四层结构（从底到顶）
| 层 | 位置 | 端口 | 职责 |
|---|------|------|------|
| 本能层 | 底部 | 有外端口 | 每tick：脉冲收发、能量管理、情感更新 |
| 认知层 | 中下 | 有外端口 | 每5-15tick：LLM分析、KG构建、别名生成 |
| 服务层 | 中上 | 有外端口 | 按需：搜索、recall、记忆创建 |
| 身份层 | 顶部 | **无外端口，密封** | 系统身份/宪法/使命，一经确认不可修改，重置才可改 |

### 端口机制
- 圆柱外表面离散化为一组端口（顶点）
- 正四面体顶点与端口合并 = 连接
- 端口不够用时圆柱自动扩展
- 身份层无外端口

### N多面体
- 由共享顶点的正四面体自然聚簇形成
- 每个N多面体通过端口与圆柱相连
- **N多面体之间互不直连** — 信息必须经过中央高塔中转

### 脉冲机制（星型拓扑，原路收回）
1. 本能层通过外端口发出脉冲
2. 脉冲进入对应N多面体，遍历正四面体收集信息
3. 脉冲**原路从同一端口收回**，携带信息
4. 收回信息通过圆柱内部通道上传认知层
5. 认知层LLM分析决策
6. 决策下发给本能层或服务层执行

### 心跳检测
- 发出N条脉冲，收回M条
- M < N → 精确定位哪个端口/簇断裂
- 可区分健康/衰减/断裂/严重损坏四个等级
- 系统有慢性病检测能力

### 记忆放置（纯物理）
1. 找最相似的已有正四面体（锚点）
2. 在锚点EDGE_LENGTH处放置
3. 顶点合并自然连接N多面体
4. N多面体通过端口连接圆柱
5. **位置由物理法则决定，分类只负责标签**
6. **分层是拓扑结果（连到圆柱哪个高度段），不是预设约束**

### 身份确认流程
1. 系统首次启动，身份层空白
2. 外部智能体首次存入记忆 → 认知层暂停
3. 向智能体请求：系统名称、使命、作者等
4. 写入身份层 + **锁定密封**
5. 通知：身份已确认不可修改，除非系统重置

## 第二轮审计已修复（在本次重构之前）
- C1: propagate()沿ChannelHop遍历
- C2: 移除Neural脉冲blend_rate参数
- C3: 别名生成0-based索引映射回实际ID
- H1: Neural脉冲top候选跳过随机过滤
- H2: api_recall find_clusters移到循环外
- H3: Fuse操作按宪法§2.6/§8.1阻止
- H4: auto_link_one O(N)替代auto_link O(N²)
- H5: api_reason_analogies使用实际KG
- H6: classifier.rs RwLock unwrap→unwrap_or_else
- 19个MEDIUM/LOW：unused imports/vars清理，删除dead code

## 15个空间违规诊断（本次重构要解决的）
1. V-01 CRITICAL: 固定半径带(layer_radius)
2. V-02 CRITICAL: 固定角度扇区(ANGLE_STEP)
3. V-03 CRITICAL: Layer由LLM分配而非拓扑计算
4. V-04 CRITICAL: find_best_placement用极坐标
5. V-05 HIGH: snap_to_share_vertex补丁
6. V-06 MEDIUM: count_vertex_merges暴力O(N²)
7. V-07 HIGH: category_target_position人工位置
8. V-08 HIGH: Z轴压平
9. V-09 CRITICAL: decision.rs物理传送四面体
10. V-10 LOW: EDGE_LENGTH重复定义
11. V-11 MEDIUM: snap_to_share_vertex魔术阈值2.0
12. V-12 HIGH: fallback_classify强制分Layer
13. V-13 LOW: dream报告假tetras_moved
14. V-14 MEDIUM: 脉冲选目标用空间距离
15. V-15 MEDIUM: MoveTetra任务存在

## Phase 1 已完成
- **文件**: C:\TetraMem-V14\src\domain\cylinder.rs
- **结构体**: Cylinder, Port, CylinderLayer, LayerZone, IdentityInfo, PulseReport, HealthReport, LayerHealth
- **关键方法**:
  - Cylinder::new() — 初始化4层，每层生成端口环
  - find_free_port/assign_port/release_port — 端口生命周期
  - expand/ensure_free_port — 自动扩展
  - health_check — 心跳检测
  - confirm_identity/reset_identity — 身份锁定/重置
  - zone_for_layer — 层高度区间查询
- **常量**: INITIAL_RADIUS=2.0, INITIAL_HEIGHT=8.0, INNER_RADIUS_RATIO=0.3, PORTS_PER_RING=8, RING_SPACING=1.0
- **Port ID**: 从1_000_000开始，避免与普通顶点冲突
- **测试**: 8 pass（initial_ports, identity_lock, identity_reset, assign_release, expand, layer_zone, health_check, layer_index）
- **总测试**: 130 pass (8 new + 122 existing)

## Phase 2-9 待实施

### Phase 2: Space集成Cylinder
- Space新增cylinder字段
- 端口作为特殊顶点参与vertex_grid
- 新增connect_to_port(tetra_id, port_id)方法
- 簇检测考虑圆柱连接

### Phase 3: 重写classifier.rs
- 删除: ANGLE_STEP, LAYER_RADIUS_*, layer_radius(), next_angle, angle from CategoryInfo, layer from ClassifyResult, category_target_position()
- 保留: 分类标签生成（LLM+fallback）
- ClassifyResult只含: category, parent
- Layer由连接到圆柱哪个高度段决定，不由分类决定

### Phase 4: 重写gateway.rs find_best_placement
- 删除: 极坐标计算, snap_to_share_vertex, count_vertex_merges
- 纯物理: 找最相似四面体→EDGE_LENGTH处放置→连接圆柱端口
- 使用crate::domain::tetra::EDGE_LENGTH（删除本地重复定义）

### Phase 5: 重写pulse.rs
- 星型拓扑: 端口发出→探索N多面体→原路收回
- PulseReport收集数据
- 心跳检测集成
- 删除旧的拓扑路由(共享顶点→KG桥接→兜底)

### Phase 6: 新建engine/identity.rs
- 身份层管理器
- 首次记忆创建时触发确认流程
- 认知层向请求方获取身份信息
- 锁定后不可修改

### Phase 7: 更新scheduler.rs
- 4层调度器(身份+本能+认知+服务)
- 脉冲心跳检测逻辑
- 删除MoveTetra任务
- 删除ScheduledTask::MoveTetra variant
- dream报告tetras_moved=0

### Phase 8: 更新routes.rs + dashboard.html
- 身份确认API端点
- 心跳状态展示
- 圆柱可视化替代簇气泡
- 4层状态展示

### Phase 9: 测试+编译+实测
- 更新所有受影响的单元测试
- cargo test --lib 全量通过
- 启动服务器实测: 创建/搜索/recall/脉冲/身份确认

## 关键文件清单
- src/domain/cylinder.rs — 新增，圆柱体核心
- src/domain/space.rs — 需修改，集成Cylinder
- src/domain/mod.rs — 已修改，注册cylinder模块
- src/engine/classifier.rs — 需重写
- src/engine/gateway.rs — 需重写find_best_placement
- src/engine/pulse.rs — 需重写
- src/engine/scheduler.rs — 需修改
- src/engine/identity.rs — 新增
- src/engine/mod.rs — 需注册identity模块
- src/engine/cognitive.rs — 需加身份确认流程
- src/engine/dream.rs — tetras_moved=0
- src/engine/decision.rs — 删除物理传送
- src/api/routes.rs — 需加身份API
- src/api/dashboard.html — 需更新可视化
- src/main.rs — 可能需加路由

## 依赖关系（实施顺序必须遵守）
Phase 1 → Phase 2 (Space需要Cylinder) → Phase 3 (classifier独立) → Phase 4 (gateway依赖新classifier+Space+Cylinder) → Phase 5 (pulse依赖新Space+Cylinder) → Phase 6 (identity独立) → Phase 7 (scheduler依赖所有) → Phase 8 (routes依赖scheduler) → Phase 9
