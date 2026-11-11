use crate::domain::space::{Cluster, Space};
use crate::engine::vector::VectorLayer;

/// 巨簇防护(七维研究-调度器维度 N=10⁵ 熵炸弹): 成对 Jaccard O(K²) 在
/// K=10⁴ 单簇时一次调用 ~5×10⁷ 集合运算, 拖垮 120s tick。
/// 超过上限时按等距抽样 CAP 个成员作无偏估计——K≤CAP 时与精确值完全一致
/// (fission 门限 0.3/0.4 的标定对小簇不变)。
const ENTROPY_SAMPLE_CAP: usize = 64;

fn sampled_ids(ids: &[crate::domain::tetra::TetraId]) -> Vec<crate::domain::tetra::TetraId> {
    if ids.len() <= ENTROPY_SAMPLE_CAP {
        return ids.to_vec();
    }
    let stride = ids.len() as f64 / ENTROPY_SAMPLE_CAP as f64;
    (0..ENTROPY_SAMPLE_CAP)
        .map(|i| ids[(i as f64 * stride) as usize])
        .collect()
}

fn mean_pairwise_dissimilarity(labels: &[&Vec<String>]) -> f64 {
    if labels.len() < 2 {
        return 0.0;
    }
    let mut total_dissimilarity = 0.0;
    let mut pairs = 0usize;
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            let sim = VectorLayer::label_jaccard(labels[i], labels[j]);
            total_dissimilarity += 1.0 - sim;
            pairs += 1;
        }
    }
    total_dissimilarity / pairs as f64
}

pub fn compute_entropy_from_labels(
    ids: &[crate::domain::tetra::TetraId],
    labels_map: &std::collections::HashMap<u64, Vec<String>>,
) -> f64 {
    if ids.len() < 2 {
        return 0.0;
    }

    // 先抽样再取标签: 巨簇只克隆 CAP 份标签, 免 O(K·L) 全量克隆
    let sampled = sampled_ids(ids);
    let labels: Vec<&Vec<String>> = sampled
        .iter()
        .filter_map(|id| labels_map.get(id))
        .collect();

    mean_pairwise_dissimilarity(&labels)
}

pub fn compute_entropy(space: &Space, cluster: &Cluster) -> f64 {
    let ids = &cluster.tetra_ids;
    if ids.len() < 2 {
        return 0.0;
    }

    let sampled = sampled_ids(ids);
    let owned: Vec<Vec<String>> = sampled
        .iter()
        .filter_map(|id| space.get_tetrahedron(*id).map(|t| t.data.labels.clone()))
        .collect();
    let labels: Vec<&Vec<String>> = owned.iter().collect();

    mean_pairwise_dissimilarity(&labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    #[test]
    fn test_split_heterogeneous_cluster() {
        let space = Space::new();

        let mut ids = Vec::new();
        for (i, (text, labels)) in [
            ("alpha beta gamma", vec!["letters".to_string()]),
            ("quantum physics relativity", vec!["physics".to_string()]),
        ]
        .iter()
        .enumerate()
        {
            let core = Point3::new(i as f64 * 3.0, 0.0, 0.0);
            let positions = Tetrahedron::compute_vertices(core);
            let tetra = Tetrahedron {
                id: 0,
                vertex_ids: [0; 4],
                core,
                data: MemoryPayload {
                    content: text.to_string(),
                    content_hash: 0,
                    labels: labels.clone(),
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
                    ..Default::default()
                },
                mass: 1.0,
            };
            let id = space.add_tetrahedron(&tetra, &positions).unwrap();
            ids.push(id);
        }

        let cluster = Cluster {
            tetra_ids: ids.clone(),
        };
        let ent = compute_entropy(&space, &cluster);
        assert!(ent > 0.0);
    }

    #[test]
    fn entropy_exact_below_sample_cap_and_approximate_above() {
        // K≤CAP: 与逐对精确值一致; K>CAP: 等距抽样的无偏估计, 误差有界
        use std::collections::HashMap;
        let mut labels_map: HashMap<u64, Vec<String>> = HashMap::new();
        let mut ids = Vec::new();
        for i in 0..200u64 {
            // 前半标签 A, 后半标签 B: 精确熵 = 跨组对比例 × 1.0 ≈ 0.5
            let label = if i < 100 { "group-a" } else { "group-b" };
            labels_map.insert(i, vec![label.to_string()]);
            ids.push(i);
        }
        let sampled = compute_entropy_from_labels(&ids, &labels_map);
        // 前 64 个全在 group-a: K≤CAP 走精确路径, 同组熵恒为 0
        let homogeneous = compute_entropy_from_labels(&ids[..64], &labels_map);
        assert!(
            homogeneous.abs() < 1e-9,
            "homogeneous cluster entropy must be exactly 0, got {homogeneous}"
        );
        // K=200>CAP 走抽样路径: 估计应接近理论值 2·(100·100)/(200·199) ≈ 0.5025
        assert!(
            (sampled - 0.5025).abs() < 0.05,
            "sampled entropy {sampled:.3} should approximate 0.5025"
        );
    }
}
