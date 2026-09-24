use crate::domain::space::{Cluster, Space};
use crate::engine::vector::VectorLayer;

pub fn compute_entropy_from_labels(
    ids: &[crate::domain::tetra::TetraId],
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
}
