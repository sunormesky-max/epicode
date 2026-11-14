//! P3-2 SR 网格码: KG 转移矩阵的谱嵌入 → 多尺度网格码 → 近似 PPR 检索。
//!
//! 神经科学背景: 内嗅皮层网格细胞以多尺度周期性放电编码空间位置。
//! Stachenfeld et al. 2017 证明 SR(Successor Representation) 的特征向量
//! 在图拓扑上呈现网格状周期模式——不同特征值对应不同"分辨率"的空间尺度。
//!
//! Epicode 应用: PPR 图搜索每次查询需要 BFS 建图 + 15 次幂迭代。
//! 谱网格码预计算转移矩阵的 top-K 特征向量, 检索退化为
//! O(K) 投影 + O(K·N) 打分——无需 BFS, 无需迭代。
//!
//! 数学: SR 方程 rank = α·seed + (1-α)·T·rank 的解可展开为
//! rank = Σᵢ ⟨seed, vᵢ⟩ / (1 - (1-α)λᵢ) · vᵢ
//! 其中 vᵢ, λᵢ 是 T 的特征对。预计算 v₁..v_K 后,
//! 任意 seed 的 PPR 可由 K 项求和近似(网格码重建)。

use crate::engine::knowledge::KnowledgeGraph;
use std::collections::HashMap;

/// 预计算特征向量数(网格尺度数)。K=32 在精度与开销间平衡:
/// PPR 15 次幂迭代 ≈ 截断到 λ₁₅ 项的特征展开。
pub const SPECTRAL_K: usize = 32;

/// 幂迭代次数(每个特征向量)。
const POWER_ITERS: usize = 50;

/// SR 转移概率(与 search_engine::PPR_RESTART 一致)。
const PPR_RESTART: f64 = 0.15;

/// 谱网格码索引。
#[derive(Debug, Clone, Default)]
pub struct SpectralIndex {
    /// Top-K 特征向量(每个是稀疏 node_id → value 映射)。
    pub basis: Vec<HashMap<u64, f64>>,
    /// 对应特征值(降序)。λ₁≈1.0 是平稳分布; v₂..v_K 是网格码。
    pub eigenvalues: Vec<f64>,
    /// 预计算的重建系数分母: 1 / (1 - (1-α)·λᵢ)
    /// PPR 展开的系数因子, 消除逐查询的除法。
    coefficients: Vec<f64>,
    /// 计算时的关系数(失效检测)。
    pub relation_count: usize,
}

/// 从 KG 提取行归一化邻接表(转移矩阵 T 的稀疏表示)。
fn build_transition(kg: &KnowledgeGraph) -> HashMap<u64, Vec<(u64, f64)>> {
    let relations = kg.all_relations();
    // 累积边权(双向对称关系两条方向都计入)
    let mut adj: HashMap<u64, HashMap<u64, f64>> = HashMap::new();
    for r in &relations {
        let symmetric = r.relation_type.is_symmetric();
        *adj.entry(r.source)
            .or_default()
            .entry(r.target)
            .or_insert(0.0) += r.strength;
        if symmetric {
            *adj.entry(r.target)
                .or_default()
                .entry(r.source)
                .or_insert(0.0) += r.strength;
        }
    }
    // 行归一化: T[u][v] = w(u,v) / Σ w(u,·)
    adj.into_iter()
        .filter_map(|(u, edges)| {
            let total: f64 = edges.values().sum();
            if total <= 0.0 {
                return None;
            }
            Some((
                u,
                edges
                    .into_iter()
                    .map(|(v, w)| (v, w / total))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect()
}

/// 幂迭代 + Gram-Schmidt 正交化, 求第 k 个特征向量。
/// T 是行随机的(λ₁=1), 从 v₂ 起做正交化即可。
fn power_iterate(
    t: &HashMap<u64, Vec<(u64, f64)>>,
    previous: &[HashMap<u64, f64>],
    seed: u64,
) -> (HashMap<u64, f64>, f64) {
    let nodes: Vec<u64> = t.keys().copied().collect();
    if nodes.is_empty() {
        return (HashMap::new(), 0.0);
    }

    // 确定性初始化: 以 seed 为脉冲, 均匀底噪
    let mut v: HashMap<u64, f64> = nodes
        .iter()
        .map(|&n| {
            (
                n,
                if n == seed {
                    1.0
                } else {
                    1.0 / (nodes.len() as f64).sqrt()
                },
            )
        })
        .collect();

    for _ in 0..POWER_ITERS {
        // v ← T·v
        let mut next: HashMap<u64, f64> = HashMap::with_capacity(v.len());
        for (&u, &vu) in &v {
            if vu.abs() < 1e-12 {
                continue;
            }
            if let Some(edges) = t.get(&u) {
                for &(w, weight) in edges {
                    *next.entry(w).or_insert(0.0) += vu * weight;
                }
            }
        }
        // Gram-Schmidt: 减去已有特征向量方向的分量
        for prev in previous {
            let dot: f64 = next
                .iter()
                .filter_map(|(&n, &x)| prev.get(&n).map(|&p| x * p))
                .sum();
            if dot.abs() > 1e-15 {
                for (&n, &p) in prev {
                    if let Some(x) = next.get_mut(&n) {
                        *x -= dot * p;
                    }
                }
            }
        }
        // 归一化(ℓ₂)
        let norm: f64 = next.values().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-12 {
            break;
        }
        for x in next.values_mut() {
            let nx = *x;
            *x = nx / norm;
        }
        v = next;
    }

    // Rayleigh 商: λ = vᵀ·T·v
    let mut tv: HashMap<u64, f64> = HashMap::new();
    for (&u, &vu) in &v {
        if vu.abs() < 1e-12 {
            continue;
        }
        if let Some(edges) = t.get(&u) {
            for &(w, weight) in edges {
                *tv.entry(w).or_insert(0.0) += vu * weight;
            }
        }
    }
    let eigenvalue: f64 = v
        .iter()
        .filter_map(|(&n, &x)| tv.get(&n).map(|&y| x * y))
        .sum();

    (v, eigenvalue)
}

impl SpectralIndex {
    /// 从 KG 计算谱网格码索引。O(K × POWER_ITERS × |E|)。
    pub fn compute(kg: &KnowledgeGraph) -> Self {
        let relation_count = kg.relation_count();
        if relation_count < 10 {
            return Self::default();
        }
        let t = build_transition(kg);
        if t.is_empty() {
            return Self::default();
        }

        // 确定性种子: 最小 node id(避免 HashMap 随机性影响)
        let seed = t.keys().copied().min().unwrap_or(0);

        let mut basis = Vec::with_capacity(SPECTRAL_K);
        let mut eigenvalues = Vec::with_capacity(SPECTRAL_K);

        for k in 0..SPECTRAL_K {
            let (v, lambda) = power_iterate(&t, &basis, seed.wrapping_add(k as u64 * 7919));
            if v.is_empty() || lambda.abs() < 1e-8 {
                break;
            }
            basis.push(v);
            eigenvalues.push(lambda);
        }

        let coefficients = eigenvalues
            .iter()
            .map(|&lambda| {
                let denom = 1.0 - (1.0 - PPR_RESTART) * lambda;
                if denom.abs() < 1e-10 {
                    1e10
                } else {
                    1.0 / denom
                }
            })
            .collect();

        Self {
            basis,
            eigenvalues,
            coefficients,
            relation_count,
        }
    }

    /// 是否可用(有足够的特征向量)。
    pub fn is_ready(&self, kg: &KnowledgeGraph) -> bool {
        self.basis.len() >= 4 && self.relation_count == kg.relation_count()
    }

    /// 将 seed 分布投影到网格码 (K 维系数)。
    pub fn project(&self, seeds: &[(u64, f64)]) -> Vec<f64> {
        let seed_map: HashMap<u64, f64> = seeds.iter().copied().collect();
        self.basis
            .iter()
            .map(|v| {
                seed_map
                    .iter()
                    .filter_map(|(&n, &s)| v.get(&n).map(|&x| s * x))
                    .sum()
            })
            .collect()
    }

    /// 从网格码重建近似 PPR 分布, 返回 top-k (node_id, score)。
    /// score = Σᵢ code[i] · coeff[i] · vᵢ[node]
    pub fn retrieve(&self, code: &[f64], top_k: usize) -> Vec<(u64, f64)> {
        if code.len() != self.basis.len() || code.is_empty() {
            return vec![];
        }
        // 预乘系数
        let weighted: Vec<f64> = code
            .iter()
            .zip(&self.coefficients)
            .map(|(&c, &k)| c * k)
            .collect();

        // 重建得分: 对每个特征向量的每个节点累加
        let mut scores: HashMap<u64, f64> = HashMap::new();
        for (i, v) in self.basis.iter().enumerate() {
            let w = weighted[i];
            if w.abs() < 1e-12 {
                continue;
            }
            for (&node, &x) in v {
                *scores.entry(node).or_insert(0.0) += w * x;
            }
        }

        let mut ranked: Vec<(u64, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        // 归一化到 [0,1] 区间(与 PPR 输出对齐)
        if let Some((_, max)) = ranked.first() {
            let m = max.abs().max(1e-9);
            for (_, s) in ranked.iter_mut() {
                *s = (s / m).clamp(0.0, 1.0);
            }
        }
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::knowledge::{KnowledgeGraph, RelationType};

    /// 构造一个双社区图: 社区A(1-2-3-4-5) + 社区B(10-11-12-13-14) + 桥(5-10)
    fn make_two_community_kg() -> KnowledgeGraph {
        let kg = KnowledgeGraph::new();
        for i in 1..=4u64 {
            kg.add_relation(i, i + 1, RelationType::SimilarTo, 0.8);
        }
        for i in 10..=13u64 {
            kg.add_relation(i, i + 1, RelationType::SimilarTo, 0.8);
        }
        kg.add_relation(5, 10, RelationType::SimilarTo, 0.3);
        kg
    }

    #[test]
    fn spectral_index_separates_communities() {
        // 网格码的核心性质: 不同社区的节点在特征空间中可区分
        let kg = make_two_community_kg();
        let idx = SpectralIndex::compute(&kg);
        assert!(
            idx.basis.len() >= 4,
            "need ≥4 eigenvectors, got {}",
            idx.basis.len()
        );
        assert!(idx.is_ready(&kg));
        assert!(
            idx.eigenvalues[0] > 0.9,
            "λ₁ ≈ 1 for stochastic matrix, got {}",
            idx.eigenvalues[0]
        );

        // seed 在社区 A → 检索结果应偏向社区 A
        let code = idx.project(&[(1u64, 1.0), (2u64, 1.0)]);
        let results = idx.retrieve(&code, 10);
        assert!(!results.is_empty());
        // 社区 A 节点应占多数(≥50%)
        let community_a_count = results
            .iter()
            .filter(|(id, score)| (1..=5).contains(id) && *score > 0.1)
            .count();
        assert!(
            community_a_count >= 3,
            "community A should dominate when seeded from A: {:?}",
            results
        );
    }

    #[test]
    fn retrieve_matches_seed_neighborhood() {
        // 从社区 B 的节点出发, 社区 B 的节点应排在前面
        let kg = make_two_community_kg();
        let idx = SpectralIndex::compute(&kg);
        let code = idx.project(&[(12u64, 1.0)]);
        let results = idx.retrieve(&code, 5);
        assert!(
            results
                .iter()
                .take(3)
                .filter(|(id, _)| (10..=14).contains(id))
                .count()
                >= 2,
            "community B nodes should rank high when seeded from B: {:?}",
            results
        );
    }

    #[test]
    fn empty_kg_returns_default() {
        let kg = KnowledgeGraph::new();
        let idx = SpectralIndex::compute(&kg);
        assert!(idx.basis.is_empty());
        assert!(!idx.is_ready(&kg));
    }
}
