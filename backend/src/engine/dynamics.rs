use crate::domain::space::{Cluster, Space};
use crate::domain::tetra::TetraId;
use crate::engine::vector::VectorLayer;

pub fn split_cluster(space: &Space, cluster: &Cluster) -> (Cluster, Cluster) {
    let labels_map: std::collections::HashMap<u64, Vec<String>> = cluster
        .tetra_ids
        .iter()
        .filter_map(|&id| {
            space
                .get_tetrahedron(id)
                .map(|t| (id, t.data.labels.clone()))
        })
        .collect();
    split_cluster_from_labels(cluster, &labels_map)
}

pub fn split_cluster_from_labels(
    cluster: &Cluster,
    labels_map: &std::collections::HashMap<u64, Vec<String>>,
) -> (Cluster, Cluster) {
    let (seed_a, seed_b) = find_seeds_from_labels(&cluster.tetra_ids, labels_map);

    let mut group_a = Vec::new();
    let mut group_b = Vec::new();

    let labels_a = labels_map.get(&seed_a).cloned().unwrap_or_default();
    let labels_b = labels_map.get(&seed_b).cloned().unwrap_or_default();

    for &id in &cluster.tetra_ids {
        if id == seed_a {
            group_a.push(id);
        } else if id == seed_b {
            group_b.push(id);
        } else {
            let labels = labels_map.get(&id).cloned().unwrap_or_default();
            let sim_a = VectorLayer::label_jaccard(&labels, &labels_a);
            let sim_b = VectorLayer::label_jaccard(&labels, &labels_b);
            if sim_a >= sim_b {
                group_a.push(id);
            } else {
                group_b.push(id);
            }
        }
    }

    (
        Cluster { tetra_ids: group_a },
        Cluster { tetra_ids: group_b },
    )
}

fn find_seeds_from_labels(
    ids: &[TetraId],
    labels_map: &std::collections::HashMap<u64, Vec<String>>,
) -> (TetraId, TetraId) {
    if ids.len() < 2 {
        return (ids[0], ids[0]);
    }

    let mut min_sim = f64::MAX;
    let mut seed_a = ids[0];
    let mut seed_b = ids[1];

    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let labels_i = labels_map.get(&ids[i]).cloned().unwrap_or_default();
            let labels_j = labels_map.get(&ids[j]).cloned().unwrap_or_default();
            let sim = VectorLayer::label_jaccard(&labels_i, &labels_j);
            if sim < min_sim {
                min_sim = sim;
                seed_a = ids[i];
                seed_b = ids[j];
            }
        }
    }

    (seed_a, seed_b)
}

pub fn should_split(space: &Space, cluster: &Cluster, threshold: f64) -> bool {
    compute_entropy(space, cluster) > threshold
}

pub fn compute_entropy_from_labels(
    ids: &[TetraId],
    labels_map: &std::collections::HashMap<u64, Vec<String>>,
) -> f64 {
    if ids.len() < 2 {
        return 0.0;
    }

    let labels: Vec<&Vec<String>> = ids.iter().filter_map(|id| labels_map.get(id)).collect();

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

pub fn compute_entropy(space: &Space, cluster: &Cluster) -> f64 {
    let ids = &cluster.tetra_ids;
    if ids.len() < 2 {
        return 0.0;
    }

    let labels: Vec<Vec<String>> = ids
        .iter()
        .filter_map(|id| space.get_tetrahedron(*id).map(|t| t.data.labels.clone()))
        .collect();

    if labels.len() < 2 {
        return 0.0;
    }

    let mut total_dissimilarity = 0.0;
    let mut pairs = 0usize;

    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            let sim = VectorLayer::label_jaccard(&labels[i], &labels[j]);
            total_dissimilarity += 1.0 - sim;
            pairs += 1;
        }
    }

    total_dissimilarity / pairs as f64
}

pub fn execute_fission(space: &Space, cluster: &Cluster) -> Result<(Cluster, Cluster), String> {
    if cluster.tetra_ids.len() < 3 {
        return Err("cluster too small to split".into());
    }
    Ok(split_cluster(space, cluster))
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
quality_score: 1.0,
memory_type: None,
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
    fn test_split_small_cluster_fails() {
        let space = Space::new();
        let cluster = Cluster {
            tetra_ids: vec![0, 1],
        };
        assert!(execute_fission(&space, &cluster).is_err());
    }
}
