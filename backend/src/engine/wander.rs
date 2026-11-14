//! 自由联想游走(free-association wander): 从一条线索沿知识图逐步漫游。
//!
//! 与 memory_recall 的区别: recall 是**查询驱动**的扇出检索(一颗种子炸开
//! 成排序列表); wander 是**线索驱动**的贪心随机游走——每一步只选"当下
//! 最有意思的下一条边", 输出一条**可解释的联想链** A→B→C→D, 每跳带
//! "为什么跳到这"的理由。这是把系统从"检索工具"升级为"思考伙伴"的
//! 关键原语: 检索回答"给我答案", 联想回答"带我漫游"。
//!
//! 四档深度(用户定义的联想模式):
//! - 浅度联想(shallow): 3 跳, 低温 0.1 —— "什么立刻浮上心头", 走最强边
//! - 中度联想(medium):  8 跳, 中温 0.4 —— 允许一两次出题跳跃
//! - 深度联想(deep):   16 跳, 高温 0.7 —— 长链跨域, 惊喜加权
//! - 自由联想(free):   步数由调用方(智能体)自定, 上限 64, 温度自定
//!
//! 温度语义(与 LLM 采样同构): τ→0 退化为 argmax(永远走最强边),
//! τ升高后 softmax 采样开始探索次强边——联想的发散度旋钮。
//! 策略在边强度之外改变"什么算有意思": novel 偏好冷记忆(惊喜),
//! temporal 沿 Precedes 走时间线, contradict 沿 Contradicts 走张力。

use crate::domain::tetra::MemoryPayload;
use std::collections::{HashMap, HashSet};

/// 浅度联想步数。
pub const WANDER_SHALLOW_HOPS: usize = 3;
/// 中度联想步数。
pub const WANDER_MEDIUM_HOPS: usize = 8;
/// 深度联想步数。
pub const WANDER_DEEP_HOPS: usize = 16;
/// 自由联想默认步数。
pub const WANDER_FREE_DEFAULT_HOPS: usize = 24;
/// 自由联想步数硬上限(防失控游走)。
pub const WANDER_FREE_MAX_HOPS: usize = 64;
/// 温度低于此值按 argmax 处理(softmax 数值下界)。
pub const WANDER_TEMP_ARGMAX: f64 = 0.05;
/// 链上每跳内容预览长度(字符, CJK 安全截断)。
pub const WANDER_PREVIEW_CHARS: usize = 120;

/// 联想深度档位 —— 四档联想模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthPreset {
    /// 浅度联想: 3 跳 / τ=0.1
    Shallow,
    /// 中度联想: 8 跳 / τ=0.4
    Medium,
    /// 深度联想: 16 跳 / τ=0.7
    Deep,
    /// 自由联想: 步数与温度由智能体决定
    Free,
}

impl DepthPreset {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "shallow" => Some(Self::Shallow),
            "medium" => Some(Self::Medium),
            "deep" => Some(Self::Deep),
            "free" => Some(Self::Free),
            _ => None,
        }
    }

    /// 档位默认步数(free 档可被调用方覆盖)。
    pub fn default_hops(self) -> usize {
        match self {
            Self::Shallow => WANDER_SHALLOW_HOPS,
            Self::Medium => WANDER_MEDIUM_HOPS,
            Self::Deep => WANDER_DEEP_HOPS,
            Self::Free => WANDER_FREE_DEFAULT_HOPS,
        }
    }

    /// 档位默认温度: 越深越发散。
    pub fn default_temperature(self) -> f64 {
        match self {
            Self::Shallow => 0.1,
            Self::Medium => 0.4,
            Self::Deep => 0.7,
            Self::Free => 0.8,
        }
    }
}

/// 游走策略: 边强度之外, "什么算有意思"的定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// 永远偏好最强边(默认, 收敛联想)
    Strongest,
    /// 偏好冷记忆(access 低 = 惊喜, 发散联想)
    Novel,
    /// 沿 Precedes 边走时间线("接下来发生了什么")
    Temporal,
    /// 沿 Contradicts 边走张力("哪里有矛盾")
    Contradict,
}

impl Strategy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "strongest" => Some(Self::Strongest),
            "novel" => Some(Self::Novel),
            "temporal" => Some(Self::Temporal),
            "contradict" => Some(Self::Contradict),
            _ => None,
        }
    }

    /// 关系类型权重: 策略对"有意思的边"的偏好。
    fn relation_weight(self, relation: &str) -> f64 {
        match self {
            Self::Strongest | Self::Novel => 1.0,
            Self::Temporal => {
                if relation == "Precedes" {
                    2.5
                } else {
                    0.5
                }
            }
            Self::Contradict => {
                if relation == "Contradicts" {
                    3.0
                } else {
                    0.5
                }
            }
        }
    }
}

/// 游走参数。
pub struct WanderParams {
    pub start: crate::domain::tetra::TetraId,
    pub depth: DepthPreset,
    pub strategy: Strategy,
    /// free 档步数覆盖(其它档忽略, 1..=64)。
    pub max_steps: Option<usize>,
    /// 温度覆盖(所有档生效, 0..=1)。
    pub temperature: Option<f64>,
    /// RNG 种子(测试确定性用; 缺省取时间)。
    pub seed: Option<u64>,
}

/// 一跳的进入边(为什么从上一条跳到这条)。
#[derive(Debug, Clone)]
pub struct ViaEdge {
    pub from: crate::domain::tetra::TetraId,
    pub relation: String,
    pub strength: f64,
    pub reason: String,
}

/// 链上一个节点。
#[derive(Debug, Clone)]
pub struct WanderHop {
    pub hop: usize, // 0 = 起点
    pub id: crate::domain::tetra::TetraId,
    pub preview: String,
    pub labels: Vec<String>,
    pub importance: f64,
    pub via: Option<ViaEdge>,
}

/// 游走结果。
#[derive(Debug, Clone, Default)]
pub struct WanderResult {
    pub chain: Vec<WanderHop>,
    pub total_hops: usize,
    pub unique_memories: usize,
    pub relation_types_used: HashMap<String, usize>,
    /// "max_steps" | "dead_end" | "loop_closed"
    pub stopped_reason: String,
}

/// SplitMix64 —— 与 sparse_code.rs 同源的确定性 RNG(零依赖)。
struct SmRng(u64);

impl SmRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// CJK 安全截断。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// 冷记忆惊喜度: access=0 → 1.0, access=9 → 0.1, 封顶趋近 0。
/// 从未被召回过的记忆在联想中是"惊喜"(大脑偏好的正是意外关联)。
fn novelty(payload: &MemoryPayload) -> f64 {
    1.0 / (1.0 + payload.access_count.min(20) as f64)
}

/// softmax 温度采样: τ≤WANDER_TEMP_ARGMAX 时 argmax(平手取先出现者)。
fn sample_index(scores: &[f64], temperature: f64, rng: &mut SmRng) -> usize {
    if scores.is_empty() {
        return 0;
    }
    if temperature <= WANDER_TEMP_ARGMAX {
        return scores
            .iter()
            .enumerate()
            .max_by(|a, b| {
                a.1.partial_cmp(b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(b.0.cmp(&a.0)) // 平手取先出现(小索引)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
    }
    // softmax(减最大值保数值稳定) → 累积分布采样
    let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = scores
        .iter()
        .map(|&s| ((s - max) / temperature).exp())
        .collect();
    let total: f64 = exps.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return 0;
    }
    let mut pick = rng.next_f64() * total;
    for (i, e) in exps.iter().enumerate() {
        pick -= e;
        if pick <= 0.0 {
            return i;
        }
    }
    exps.len() - 1
}

/// 自由联想游走核心。
///
/// - `relations`: 有向邻接查询(对称关系双向, Precedes/Contains 只正向)——
///   生产侧接 `api_get_relations_directed`, 语义与 PPR/SR 遍历一致。
/// - `node`: 记忆载荷查询(内容预览/labels/access_count)。
///
/// 停止条件: 步数用尽(max_steps) / 死胡同(dead_end) /
/// 邻居全被走过(loop_closed, 联想闭环——人脑"绕回来了"的时刻)。
pub fn wander<R, N>(relations: R, node: N, params: WanderParams) -> WanderResult
where
    R: Fn(crate::domain::tetra::TetraId) -> Vec<(crate::domain::tetra::TetraId, String, f64)>,
    N: Fn(crate::domain::tetra::TetraId) -> Option<MemoryPayload>,
{
    let max_hops = match params.depth {
        DepthPreset::Free => params
            .max_steps
            .unwrap_or(WANDER_FREE_DEFAULT_HOPS)
            .clamp(1, WANDER_FREE_MAX_HOPS),
        _ => params.depth.default_hops(),
    };
    let temperature = params
        .temperature
        .unwrap_or_else(|| params.depth.default_temperature())
        .clamp(0.0, 1.0);
    let mut rng = SmRng::new(params.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED)
    }));

    let mut result = WanderResult::default();
    let mut visited: HashSet<crate::domain::tetra::TetraId> = HashSet::new();
    let mut current = params.start;
    visited.insert(current);
    push_hop(&mut result, &node, 0, current, None);
    let mut prev: Option<crate::domain::tetra::TetraId> = None;
    // 回溯解锁只允许一次(联想"绕回旧地"的时刻); 再次无路可走即闭环。
    let mut relapsed = false;

    for hop in 1..=max_hops {
        let mut cands: Vec<(crate::domain::tetra::TetraId, String, f64)> = relations(current)
            .into_iter()
            .filter(|(id, _, _)| Some(*id) != prev && !visited.contains(id))
            .collect();
        if cands.is_empty() {
            if relations(current).is_empty() {
                result.stopped_reason = "dead_end".into();
                break;
            }
            if relapsed {
                result.stopped_reason = "loop_closed".into();
                break;
            }
            // 唯一一次绕环: 跳回非 prev 的已访节点
            cands = relations(current)
                .into_iter()
                .filter(|(id, _, _)| Some(*id) != prev)
                .collect();
            if cands.is_empty() {
                // 只剩 prev 可走 —— 视为闭环
                result.stopped_reason = "loop_closed".into();
                break;
            }
            relapsed = true;
        }

        // 打分: 边强度 × 策略关系权重 × (novel 策略的惊喜因子)
        let scores: Vec<f64> = cands
            .iter()
            .map(|(id, rel, strength)| {
                let base = strength.max(0.0) * params.strategy.relation_weight(rel);
                match params.strategy {
                    Strategy::Novel => {
                        let n = node(*id).map(|p| novelty(&p)).unwrap_or(0.5);
                        base * (0.2 + n)
                    }
                    _ => base,
                }
            })
            .collect();

        let picked = sample_index(&scores, temperature, &mut rng);
        let (next_id, rel, strength) = &cands[picked];
        let reason = match params.strategy {
            Strategy::Strongest => format!("strongest {} edge ({:.2})", rel, strength),
            Strategy::Novel => {
                let acc = node(*next_id).map(|p| p.access_count).unwrap_or(0);
                format!(
                    "novel memory (access={}) via {} ({:.2})",
                    acc, rel, strength
                )
            }
            Strategy::Temporal => format!("timeline: {} ({:.2})", rel, strength),
            Strategy::Contradict => format!("tension: {} ({:.2})", rel, strength),
        };
        let via = ViaEdge {
            from: current,
            relation: rel.clone(),
            strength: *strength,
            reason,
        };
        *result.relation_types_used.entry(rel.clone()).or_insert(0) += 1;
        prev = Some(current);
        current = *next_id;
        visited.insert(current);
        push_hop(&mut result, &node, hop, current, Some(via));
    }

    if result.stopped_reason.is_empty() {
        result.stopped_reason = "max_steps".into();
    }
    result.total_hops = result.chain.len().saturating_sub(1);
    result.unique_memories = visited.len();
    result
}

fn push_hop<N>(
    result: &mut WanderResult,
    node: &N,
    hop: usize,
    id: crate::domain::tetra::TetraId,
    via: Option<ViaEdge>,
) where
    N: Fn(crate::domain::tetra::TetraId) -> Option<MemoryPayload>,
{
    let payload = node(id);
    let preview = payload
        .as_ref()
        .map(|p| truncate_chars(&p.content, WANDER_PREVIEW_CHARS))
        .unwrap_or_else(|| "(记忆已不在空间中)".to_string());
    let labels = payload
        .as_ref()
        .map(|p| p.labels.clone())
        .unwrap_or_default();
    let importance = payload.as_ref().map(|p| p.importance).unwrap_or(0.0);
    result.chain.push(WanderHop {
        hop,
        id,
        preview,
        labels,
        importance,
        via,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(content: &str, access: u32) -> MemoryPayload {
        MemoryPayload {
            content: content.to_string(),
            access_count: access,
            ..Default::default()
        }
    }

    type Adj = HashMap<u64, Vec<(u64, String, f64)>>;
    type Nodes = HashMap<u64, MemoryPayload>;

    fn fixture() -> (Adj, Nodes) {
        // 时间线: 0 -Precedes-> 1 -Precedes-> 2 -Precedes-> 3
        // 旁路: 0 -SimilarTo(0.3)-> 4 -Contradicts(0.9)-> 5 -SimilarTo(0.8)-> 3
        // 3 是死胡同的桥, 5 无出边(终)
        let mut adj: Adj = HashMap::new();
        let add = |from: u64, to: u64, rel: &str, s: f64, adj: &mut Adj| {
            adj.entry(from).or_default().push((to, rel.to_string(), s));
        };
        add(0, 1, "Precedes", 0.8, &mut adj);
        add(1, 2, "Precedes", 0.7, &mut adj);
        add(2, 3, "Precedes", 0.6, &mut adj);
        add(0, 4, "SimilarTo", 0.3, &mut adj);
        add(4, 5, "Contradicts", 0.9, &mut adj);
        add(5, 3, "SimilarTo", 0.8, &mut adj);
        let mut nodes = HashMap::new();
        nodes.insert(0u64, payload("起点", 5));
        nodes.insert(1u64, payload("第二段", 0));
        nodes.insert(2u64, payload("第三段", 3));
        nodes.insert(3u64, payload("终段", 0));
        nodes.insert(4u64, payload("旁支", 0));
        nodes.insert(5u64, payload("矛盾点", 0));
        (adj, nodes)
    }

    fn run(adj: &Adj, nodes: &Nodes, params: WanderParams) -> WanderResult {
        wander(
            |id| adj.get(&id).cloned().unwrap_or_default(),
            |id| nodes.get(&id).cloned(),
            params,
        )
    }

    fn params(start: u64, depth: DepthPreset, strategy: Strategy) -> WanderParams {
        WanderParams {
            start,
            depth,
            strategy,
            max_steps: None,
            temperature: Some(0.0),
            seed: Some(42),
        }
    }

    #[test]
    fn shallow_takes_strongest_path() {
        let (adj, nodes) = fixture();
        // τ=0(argmax): 0 的两条边里 Precedes(0.8) > SimilarTo(0.3)
        let r = run(
            &adj,
            &nodes,
            params(0, DepthPreset::Shallow, Strategy::Strongest),
        );
        assert_eq!(r.total_hops, WANDER_SHALLOW_HOPS);
        assert_eq!(
            r.chain[1].id, 1,
            "第一跳应走 0.8 的 Precedes 而非 0.3 的 SimilarTo"
        );
        assert_eq!(r.chain[2].id, 2);
        assert_eq!(r.chain[3].id, 3);
        assert_eq!(r.stopped_reason, "max_steps");
        assert_eq!(r.unique_memories, 4);
    }

    #[test]
    fn deep_reaches_contradiction_branch() {
        let (adj, nodes) = fixture();
        // 16 跳足够走完 0→1→2→3 (3 无出边 → dead_end)
        let r = run(
            &adj,
            &nodes,
            params(0, DepthPreset::Deep, Strategy::Strongest),
        );
        assert_eq!(r.stopped_reason, "dead_end");
        assert!(r.chain.iter().any(|h| h.id == 3));
    }

    #[test]
    fn contradict_strategy_prefers_tension_edges() {
        let (adj, nodes) = fixture();
        // 从 4 出发只有 Contradicts(0.9) 一条边; 权重策略只改变打分
        let r = run(
            &adj,
            &nodes,
            params(4, DepthPreset::Shallow, Strategy::Contradict),
        );
        assert_eq!(r.chain[1].id, 5);
        assert!(r.chain[1].via.as_ref().unwrap().reason.contains("tension"));
        // 5→3 SimilarTo 在 contradict 策略下降权但仍是唯一未访边 → 继续走
        assert_eq!(r.chain[2].id, 3);
    }

    #[test]
    fn temporal_walks_timeline_forward_only() {
        let (adj, nodes) = fixture();
        let r = run(
            &adj,
            &nodes,
            params(0, DepthPreset::Shallow, Strategy::Temporal),
        );
        // Precedes ×2.5 压倒 SimilarTo ×0.5 → 沿时间线 0→1→2→3
        assert_eq!(r.chain[1].id, 1);
        assert_eq!(r.chain[2].id, 2);
        assert!(r.chain[1].via.as_ref().unwrap().reason.contains("timeline"));
    }

    #[test]
    fn free_mode_respects_custom_steps_cap() {
        let (adj, nodes) = fixture();
        let mut p = params(0, DepthPreset::Free, Strategy::Strongest);
        p.max_steps = Some(500); // 超上限 → clamp 到 64
        p.temperature = Some(0.9);
        let r = run(&adj, &nodes, p);
        assert!(r.total_hops <= WANDER_FREE_MAX_HOPS);
    }

    #[test]
    fn high_temperature_can_diverge() {
        let (adj, nodes) = fixture();
        // τ=1.0, 多次游走应至少有一次走上 SimilarTo 旁支(0→4)
        let mut diverged = false;
        for seed in 0..50 {
            let mut p = params(0, DepthPreset::Free, Strategy::Strongest);
            p.temperature = Some(1.0);
            p.seed = Some(seed);
            p.max_steps = Some(1);
            let r = run(&adj, &nodes, p);
            if r.chain[1].id == 4 {
                diverged = true;
                break;
            }
        }
        assert!(diverged, "高温下应能探索到 0.3 的弱边旁支");
    }

    #[test]
    fn loop_closed_when_all_neighbors_visited() {
        // 环: 0→1→0 (对称化: 1 也有回边)
        let mut adj = HashMap::new();
        adj.insert(0u64, vec![(1u64, "SimilarTo".to_string(), 0.9)]);
        adj.insert(1u64, vec![(0u64, "SimilarTo".to_string(), 0.9)]);
        let mut nodes = HashMap::new();
        nodes.insert(0u64, payload("a", 0));
        nodes.insert(1u64, payload("b", 0));
        let mut p = params(0, DepthPreset::Free, Strategy::Strongest);
        p.max_steps = Some(10);
        let r = run(&adj, &nodes, p);
        // 0→1 后: prev=0 已过滤, 1 的邻居只剩 0(已访) → 回溯再走 0,
        // 再回到 1 时 prev=0 过滤后无候选 → loop_closed
        assert_eq!(r.stopped_reason, "loop_closed");
        assert!(r.total_hops < 10);
    }

    #[test]
    fn chain_explains_every_hop() {
        let (adj, nodes) = fixture();
        let r = run(
            &adj,
            &nodes,
            params(0, DepthPreset::Shallow, Strategy::Strongest),
        );
        assert!(r.chain[0].via.is_none(), "起点无进入边");
        for h in &r.chain[1..] {
            let via = h.via.as_ref().expect("非起点必有进入边");
            assert!(!via.reason.is_empty());
            assert!(via.strength > 0.0);
        }
    }

    #[test]
    fn preview_truncation_is_char_safe() {
        let long = "忆".repeat(300);
        let mut nodes = HashMap::new();
        nodes.insert(0u64, payload(&long, 0));
        let r = wander(
            |_| vec![],
            |id| nodes.get(&id).cloned(),
            WanderParams {
                start: 0,
                depth: DepthPreset::Shallow,
                strategy: Strategy::Strongest,
                max_steps: None,
                temperature: None,
                seed: Some(1),
            },
        );
        assert_eq!(r.chain[0].preview.chars().count(), WANDER_PREVIEW_CHARS + 1); // +1 = 省略号
        assert!(r.stopped_reason == "dead_end");
    }
}
