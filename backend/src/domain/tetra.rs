use super::vertex::{Point3, VertexId};

pub type TetraId = u64;

/// Lightweight metadata for snapshots — excludes embedding vector (8KB savings/tetra).
#[derive(Debug, Clone)]
pub struct TetraMeta {
    pub id: TetraId,
    pub core: Point3,
    pub mass: f64,
    pub content: String,
    pub content_hash: u64,
    pub labels: Vec<String>,
    pub importance: f64,
    pub enforced: bool,
    pub access_count: u32,
    pub timestamp: i64,
}

pub const VERTEX_COUNT: usize = 4;
pub const EDGE_COUNT: usize = 6;
pub const FACE_COUNT: usize = 4;

/// Fixed edge length for every tetrahedron in this space.
/// The space is a uniform grid built on this constant.
pub const EDGE_LENGTH: f64 = 1.0;

const SHAPE_EPSILON: f64 = 1e-10;

/// Canonical vertex offsets from the tetrahedron center.
/// All 6 edges equal EDGE_LENGTH. Verified by construction:
///
///   v0 = ( a/2,    0,  -a/(2\u{221a}2) )
///   v1 = (-a/2,    0,  -a/(2\u{221a}2) )
///   v2 = (   0,  a/2,   a/(2\u{221a}2) )
///   v3 = (   0, -a/2,   a/(2\u{221a}2) )
///
/// where a = EDGE_LENGTH, a/(2\u{221a}2) = a \u{221a}2 / 4 \u{2248} 0.353553...
const fn canonical_offsets() -> [Point3; 4] {
    let half = 0.5;
    let z = 0.3535533905932738;
    [
        Point3 {
            x: half,
            y: 0.0,
            z: -z,
        },
        Point3 {
            x: -half,
            y: 0.0,
            z: -z,
        },
        Point3 { x: 0.0, y: half, z },
        Point3 {
            x: 0.0,
            y: -half,
            z,
        },
    ]
}

const EDGES: [(usize, usize); EDGE_COUNT] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

const FACES: [[usize; 3]; FACE_COUNT] = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryPayload {
    pub content: String,
    pub content_hash: u64,
    pub labels: Vec<String>,
    pub timestamp: i64,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub embedding: Vec<f64>,
    #[serde(default = "default_importance")]
    pub importance: f64,
    #[serde(default)]
    pub enforced: bool,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub memory_type: Option<String>,
    #[serde(default)]
    pub identity_stamp: Option<String>,
    #[serde(default)]
    pub source_agent: Option<String>,
    #[serde(default)]
    pub valid_from: i64,
    #[serde(default)]
    pub valid_to: Option<i64>,
    /// 能力B: Zep 双时序模型 — 事实实际失效的发生时间。
    /// 与 valid_to 的区别：valid_to 是"不再有效的时间点"，
    /// expired_at 是"事实本身在现实中失效的时间"（可能早于系统得知）。
    /// 例：用户 1 月换了工作，3 月才告诉 AI → expired_at=1月, invalidated_at=3月。
    #[serde(default)]
    pub expired_at: Option<i64>,
    /// 能力B: 系统得知该事实失效的时间（矛盾检测/用户纠正触发）。
    #[serde(default)]
    pub invalidated_at: Option<i64>,
    /// P1记忆分层：permanent（长期知识）/ session（会话上下文）/ bridge（桥接临时）
    /// permanent: 永不自动衰减，用户主动创建的知识
    /// session: 会话级，governor 自动过期（默认7天）
    /// bridge: 桥接临时记忆，最快过期（默认1天）
    #[serde(default)]
    pub memory_class: Option<String>,
    /// 突破3: 遗忘曲线 — 最后复习时间（搜索命中/access时更新）。
    /// None 时 fallback 到 timestamp（创建年龄）。
    /// governor effective_importance 按此值算衰减，而非创建年龄。
    #[serde(default)]
    pub last_reviewed_ts: Option<i64>,
}

fn default_importance() -> f64 {
    1.0
}

/// 派生重要性的每日衰减率(时间项)——与历史 governor 标定 0.98 保持一致。
pub const IMPORTANCE_DAILY_DECAY: f64 = 0.98;

impl MemoryPayload {
    /// P1-3 派生重要性(不落盘): R(t) = (base + ln(access)·0.3) · 0.98^review_days
    ///
    /// 单一真相的纯函数——时间与检索历史动力学全部在读侧推导:
    /// - 复习(last_reviewed_ts, 由检索回写/冷复审 KEEP 更新)重置衰减节拍;
    /// - 检索历史经 ln 放大稳定性(间隔重复效应: 常被用到的记忆衰减更慢);
    /// - base(importance 字段)只被**语义事件**改写: 创建定级 / 冷复审 KEEP /
    ///   supersede 降籍 / 隔离降籍——时间衰减不再写回。
    ///
    /// 为什么不落盘: 旧 governor apply_decay 把派生值写回 base, 下一周期又对
    /// 已衰减的 base 再乘全期衰减因子——复利化重复衰减, 曲线越走越陡
    /// (七维研究-生命周期维度: 三套写入器互相覆盖的根因)。
    /// 检索读侧应 clamp 到活地板(IMPORTANCE_FLOOR); 生命周期判定(隔离/墓碑)
    /// 允许低于地板——与宪法一致(可降籍, 不可被检索除名; 除名走 valid_to 例外)。
    pub fn derived_importance(&self) -> f64 {
        let stability_bonus = (self.access_count as f64).ln().max(0.0) * 0.3;
        let review_days = {
            let review_ts = self.last_reviewed_ts.unwrap_or(self.timestamp);
            ((chrono::Utc::now().timestamp() - review_ts) / 86400).max(0)
        };
        let decay = if review_days > 0 {
            IMPORTANCE_DAILY_DECAY.powi(review_days as i32)
        } else {
            1.0
        };
        (self.importance + stability_bonus) * decay
    }
}

impl Default for MemoryPayload {
    fn default() -> Self {
        Self {
            content: String::new(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
            identity_stamp: None,
            source_agent: None,
            valid_from: 0,
            valid_to: None,
            expired_at: None,
            invalidated_at: None,
            memory_class: None,
            last_reviewed_ts: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tetrahedron {
    pub id: TetraId,
    pub vertex_ids: [VertexId; VERTEX_COUNT],
    pub core: Point3,
    pub data: MemoryPayload,
    pub mass: f64,
}

impl Tetrahedron {
    /// Compute the 4 vertex positions for a tetrahedron centered at `center`.
    pub fn compute_vertices(center: Point3) -> [Point3; VERTEX_COUNT] {
        let offsets = canonical_offsets();
        [
            Point3::new(
                center.x + offsets[0].x,
                center.y + offsets[0].y,
                center.z + offsets[0].z,
            ),
            Point3::new(
                center.x + offsets[1].x,
                center.y + offsets[1].y,
                center.z + offsets[1].z,
            ),
            Point3::new(
                center.x + offsets[2].x,
                center.y + offsets[2].y,
                center.z + offsets[2].z,
            ),
            Point3::new(
                center.x + offsets[3].x,
                center.y + offsets[3].y,
                center.z + offsets[3].z,
            ),
        ]
    }

    pub fn edges() -> &'static [(usize, usize); EDGE_COUNT] {
        &EDGES
    }

    pub fn faces() -> &'static [[usize; 3]; FACE_COUNT] {
        &FACES
    }

    /// Get a vertex id by index (0..3).
    pub fn vertex(&self, index: usize) -> VertexId {
        self.vertex_ids[index]
    }

    /// Validate that 4 positions form a regular tetrahedron with edge = EDGE_LENGTH.
    pub fn validate_shape(positions: &[Point3; VERTEX_COUNT]) -> bool {
        for &(i, j) in Self::edges() {
            if (positions[i].distance_to(&positions[j]) - EDGE_LENGTH).abs() > SHAPE_EPSILON {
                return false;
            }
        }
        true
    }

    /// Verify that the computed centroid matches the core point.
    pub fn verify_core(positions: &[Point3; VERTEX_COUNT], core: Point3) -> bool {
        let refs: [&Point3; 4] = [&positions[0], &positions[1], &positions[2], &positions[3]];
        Point3::centroid(&refs).distance_to(&core) < SHAPE_EPSILON
    }

    /// The volume of a regular tetrahedron: V = a\u{b3}\u{221a}2 / 12.
    pub fn volume() -> f64 {
        EDGE_LENGTH.powi(3) * std::f64::consts::SQRT_2 / 12.0
    }

    /// Distance from center to any vertex: a\u{221a}6 / 4.
    pub fn center_to_vertex() -> f64 {
        EDGE_LENGTH * (6.0_f64).sqrt() / 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(center: Point3) -> [Point3; 4] {
        Tetrahedron::compute_vertices(center)
    }

    #[test]
    fn derived_importance_is_pure_and_decays_with_review_age() {
        // P1-3: 纯函数——同状态两次求值相同(不落盘 ⇒ 无复利化重复衰减)
        let now = chrono::Utc::now().timestamp();
        let mut p = MemoryPayload {
            importance: 2.0,
            timestamp: now - 86400 * 100,
            last_reviewed_ts: Some(now - 86400 * 100),
            ..Default::default()
        };
        let d1 = p.derived_importance();
        let d2 = p.derived_importance();
        assert!((d1 - d2).abs() < 1e-12, "pure function: {d1} vs {d2}");
        assert!(d1 < 2.0, "100 unaccessed days must decay: {d1}");

        // 复习重置节拍: last_reviewed_ts 拉近 → 派生值回升
        p.last_reviewed_ts = Some(now - 86400);
        let reviewed = p.derived_importance();
        assert!(
            reviewed > d1,
            "recent review resets decay cadence: {reviewed} vs {d1}"
        );

        // 检索历史放大稳定性(ln): 高访问记忆比零访问衰减少
        let mut hot = p.clone();
        hot.access_count = 50;
        assert!(
            hot.derived_importance() > p.derived_importance(),
            "access history amplifies stability (spacing effect)"
        );
    }

    #[test]
    fn all_six_edges_equal() {
        let pos = positions(Point3::zero());
        for &(i, j) in Tetrahedron::edges() {
            let d = pos[i].distance_to(&pos[j]);
            assert!(
                (d - EDGE_LENGTH).abs() < 1e-10,
                "edge ({},{}) = {}, expected {}",
                i,
                j,
                d,
                EDGE_LENGTH
            );
        }
    }

    #[test]
    fn validate_regular() {
        assert!(Tetrahedron::validate_shape(&positions(Point3::zero())));
    }

    #[test]
    fn validate_deformed() {
        let mut pos = positions(Point3::zero());
        pos[0].x += 0.5;
        assert!(!Tetrahedron::validate_shape(&pos));
    }

    #[test]
    fn core_at_centroid() {
        let c = Point3::new(5.0, -3.0, 7.0);
        assert!(Tetrahedron::verify_core(&positions(c), c));
    }

    #[test]
    fn vertex_distance_from_center() {
        let expected = Tetrahedron::center_to_vertex();
        let pos = positions(Point3::zero());
        for p in &pos {
            let d = p.distance_to(&Point3::zero());
            assert!(
                (d - expected).abs() < 1e-10,
                "vertex-center dist = {}, expected {}",
                d,
                expected
            );
        }
    }

    #[test]
    fn faces_are_equilateral() {
        let pos = positions(Point3::zero());
        for &face in Tetrahedron::faces() {
            let d01 = pos[face[0]].distance_to(&pos[face[1]]);
            let d12 = pos[face[1]].distance_to(&pos[face[2]]);
            let d20 = pos[face[2]].distance_to(&pos[face[0]]);
            assert!((d01 - EDGE_LENGTH).abs() < 1e-10);
            assert!((d12 - EDGE_LENGTH).abs() < 1e-10);
            assert!((d20 - EDGE_LENGTH).abs() < 1e-10);
        }
    }

    #[test]
    fn volume_correct() {
        let expected = EDGE_LENGTH.powi(3) * std::f64::consts::SQRT_2 / 12.0;
        assert!((expected - 0.1178511301977579).abs() < 1e-10);
    }

    #[test]
    fn translate_preserves_shape() {
        let c = Point3::new(10.0, 20.0, 30.0);
        let pos = positions(c);
        assert!(Tetrahedron::validate_shape(&pos));
        assert!(Tetrahedron::verify_core(&pos, c));
    }

    #[test]
    fn edges_count() {
        assert_eq!(Tetrahedron::edges().len(), 6);
    }

    #[test]
    fn faces_count() {
        assert_eq!(Tetrahedron::faces().len(), 4);
    }

    #[test]
    fn volume_known_value() {
        assert!((Tetrahedron::volume() - 0.1178511301977579).abs() < 1e-10);
    }
}
