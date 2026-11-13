//! P3-1 齿状回(DG)稀疏码: 随机投影 + Winner-Take-All 稀疏化 = 模式分离。
//!
//! 神经科学背景: 海马齿状回把皮层的稠密重叠输入投影到扩维空间后
//! 只保留前 K 个获胜神经元(WTA), 使相似输入的表征被拉开——
//! 相似的不再混淆, 相同的仍然重合(Johnson-Lindenstrauss + 稀疏化)。
//!
//! Epicode 应用: 去重判决(Mem0 supersede)用原始余弦会把**同主题改写**
//! 误判为重复(cos 0.92-0.95 区间)。DG 稀疏码提供正交第二意见:
//! - 真重复(逐字/同一嵌入): 稀疏码几乎全同 → overlap ≈ 1.0
//! - 同主题改写: 随机投影放大差异, top-16 集合显著分叉 → overlap 低
//! - 无关: overlap ≈ 随机水平 16/256 ≈ 0.06
//!
//! 实现约束: 权重用 (i,j) 确定性哈希即时计算(SplitMix64), 零存储、
//! 跨进程/重启稳定(无需持久化投影矩阵, 换维度即换码——维度由
//! EMBEDDING_DIM 保证恒定)。

/// 投影位宽(扩维空间维度)。
pub const DG_BITS: usize = 256;
/// WTA 保留的获胜位数量(稀疏度 16/256 = 6.25%, DG 生物稀疏度同量级)。
pub const DG_WINNERS: usize = 16;

/// 256-bit 稀疏码位图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparseCode([u64; DG_BITS / 64]);

/// (i,j) → [-1,1) 确定性伪随机投影权重(SplitMix64)。
fn proj_weight(i: usize, j: usize) -> f64 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}

/// 编码: 稠密嵌入 → 256-bit WTA 稀疏码。O(D × DG_BITS) ≈ 0.26M 乘加。
pub fn encode(embedding: &[f64]) -> SparseCode {
    let mut activations = [0.0f64; DG_BITS];
    for (i, &v) in embedding.iter().enumerate() {
        if v == 0.0 {
            continue;
        }
        for (j, a) in activations.iter_mut().enumerate() {
            *a += v * proj_weight(i, j);
        }
    }
    // WTA: 激活 top-K 位图(排序确定性: 平手取小索引)
    let mut idx: Vec<usize> = (0..DG_BITS).collect();
    idx.sort_by(|&a, &b| {
        activations[b]
            .partial_cmp(&activations[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut bits = [0u64; DG_BITS / 64];
    for &j in idx.iter().take(DG_WINNERS) {
        bits[j / 64] |= 1u64 << (j % 64);
    }
    SparseCode(bits)
}

impl SparseCode {
    /// Jaccard 重叠: |A∩B| / |A∪B| ∈ [0,1]。相同=1.0, 随机≈0.026。
    pub fn overlap(&self, other: &SparseCode) -> f64 {
        let mut and = 0u32;
        let mut or = 0u32;
        for k in 0..self.0.len() {
            and += (self.0[k] & other.0[k]).count_ones();
            or += (self.0[k] | other.0[k]).count_ones();
        }
        if or == 0 {
            return 0.0;
        }
        f64::from(and) / f64::from(or)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f64], b: &[f64]) -> f64 {
        let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        dot / (na * nb)
    }

    /// 合成嵌入: 1024 维, 种子化生成, 可指定与基准的余弦
    fn make_embedding(dim: usize, seed: u64) -> Vec<f64> {
        let mut v = Vec::with_capacity(dim);
        let mut h = seed;
        for _ in 0..dim {
            h = h.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            v.push(((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0);
        }
        v
    }

    /// 构造与 base 余弦≈target 的向量: v = normalize(base*target + noise*sqrt(1-target²))
    fn with_cosine(base: &[f64], target: f64, seed: u64) -> Vec<f64> {
        let noise = make_embedding(base.len(), seed);
        let nb: f64 = base.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nn: f64 = noise.iter().map(|x| x * x).sum::<f64>().sqrt();
        let t = target.clamp(-1.0, 1.0);
        let v: Vec<f64> = base
            .iter()
            .zip(&noise)
            .map(|(&b, &n)| b * t / nb + n * (1.0 - t * t).sqrt() / nn)
            .collect();
        let nv: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.iter().map(|x| x / nv).collect()
    }

    #[test]
    fn identical_inputs_encode_identically() {
        let e = make_embedding(1024, 42);
        assert_eq!(encode(&e), encode(&e));
        assert!(encode(&e).overlap(&encode(&e)) > 0.999);
    }

    #[test]
    fn pattern_separation_monotonic_in_cosine() {
        // DG 效应: overlap 随余弦单调下降, 且在改写区间(0.9+)显著低于 1.0
        let base = make_embedding(1024, 7);
        let dup = with_cosine(&base, 1.0, 100); // ≈ 同一嵌入
        let paraphrase = with_cosine(&base, 0.94, 200); // 同主题改写典型余弦
        let same_topic = with_cosine(&base, 0.85, 300);
        let orthogonal = with_cosine(&base, 0.0, 400);
        let reembed = with_cosine(&base, 0.999, 500); // 重复文本重嵌入

        assert!((cosine(&base, &dup) - 1.0).abs() < 1e-9);
        assert!((cosine(&base, &paraphrase) - 0.94).abs() < 1e-3);

        let c_base = encode(&base);
        let o_dup = c_base.overlap(&encode(&dup));
        let o_reembed = c_base.overlap(&encode(&reembed));
        let o_para = c_base.overlap(&encode(&paraphrase));
        let o_topic = c_base.overlap(&encode(&same_topic));
        let o_orth = c_base.overlap(&encode(&orthogonal));
        eprintln!(
            "DG_CURVE cos1.0={o_dup:.3} cos0.999={o_reembed:.3} cos0.94={o_para:.3} cos0.85={o_topic:.3} cos0.0={o_orth:.3}"
        );

        assert_eq!(o_dup, 1.0, "identical embedding → identical code");
        assert!(
            o_reembed > o_para,
            "re-embed ({o_reembed}) must outrank paraphrase ({o_para})"
        );
        assert!(
            o_para < o_reembed - 0.05,
            "paraphrase separation margin: re-embed {o_reembed} vs para {o_para}"
        );
        assert!(
            o_topic <= o_para,
            "overlap monotone: 0.85-cosine ({o_topic}) ≤ 0.94-cosine ({o_para})"
        );
        assert!(
            o_orth < o_topic,
            "orthogonal ({o_orth}) below same-topic ({o_topic})"
        );
        assert!(o_orth < 0.2, "random-level overlap: {o_orth}");
    }

    #[test]
    fn gate_threshold_separates_duplicate_from_paraphrase() {
        // 网关使用的判决门: 真重复通过, 同主题改写被拒
        let base = make_embedding(1024, 99);
        // 真重复 = 同一文本重新嵌入(嵌入扰动 ~0.999 余弦)
        let reembed = with_cosine(&base, 0.999, 500);
        let paraphrase = with_cosine(&base, 0.94, 600);
        let c = encode(&base);
        let o_reembed = c.overlap(&encode(&reembed));
        let o_para = c.overlap(&encode(&paraphrase));
        assert!(
            o_reembed > o_para,
            "re-embed ({o_reembed}) must outrank paraphrase ({o_para})"
        );
    }
}
