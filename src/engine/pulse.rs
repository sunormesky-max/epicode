use std::collections::{HashMap, HashSet, VecDeque};

use crate::domain::cylinder::{CylinderLayer, PulseReport};
use crate::domain::pulse::{Pulse, PulseResult};
use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::domain::vertex::VertexId;
use crate::engine::knowledge::KnowledgeGraph;

#[derive(Debug, Clone, Copy)]
pub enum PulseType {
    Reinforcing { boost: f64 },
    Exploratory { curiosity: f64 },
    Cascade { branch_limit: usize },
    Neural { temperature: f64 },
}

pub struct PulseEngine;

impl PulseEngine {
    pub fn send(
        space: &Space,
        kg: &KnowledgeGraph,
        pulse_type: PulseType,
        origin: TetraId,
        ttl: u32,
    ) -> Result<PulseResult, String> {
        let all = space.all_tetrahedrons();
        let snapshot: HashMap<TetraId, &crate::domain::tetra::Tetrahedron> =
            all.iter().map(|t| (t.id, t)).collect();

        let origin_tetra = snapshot.get(&origin)
            .ok_or("origin not found")?;

        let mut visited_set: HashSet<TetraId> = HashSet::new();
        visited_set.insert(origin);
        let mut visited = vec![origin];
        let mut collected_hashes: Vec<u64> = vec![origin_tetra.data.content_hash];
        let mut discoveries: Vec<(TetraId, TetraId, f64)> = Vec::new();
        let mut mass_updates: Vec<(TetraId, f64)> = Vec::new();

        let port_vid = space.port_vertex_of_tetra(origin);

        let cluster_tetras = if let Some(pvid) = port_vid {
            Self::bfs_cluster_from_port(space, pvid, ttl as usize)
        } else {
            space.bfs_neighbors(origin, ttl as usize)
        };

        for (tid, hop) in &cluster_tetras {
            if !visited_set.insert(*tid) { continue; }
            visited.push(*tid);

            if let Some(nt) = snapshot.get(tid) {
                collected_hashes.push(nt.data.content_hash);

                let sim = crate::engine::vector::VectorLayer::best_similarity(
                    &origin_tetra.data.embedding, &origin_tetra.data.labels,
                    &nt.data.embedding, &nt.data.labels,
                );

                if *hop <= 1 && sim > 0.3 {
                    discoveries.push((origin, *tid, sim));
                }

                mass_updates.push((*tid, 0.02 * sim.max(0.1)));

                kg.add_relation(origin, *tid,
                    crate::engine::knowledge::RelationType::Related, sim.max(0.1));
            }
        }

        let boost = match pulse_type {
            PulseType::Reinforcing { boost } => boost,
            _ => 0.0,
        };

        if boost > 0.0 && visited.len() > 1 {
            for i in 0..visited.len() {
                for j in (i + 1)..visited.len() {
                    let a = visited[i];
                    let b = visited[j];
                    if let (Some(ta), Some(tb)) = (snapshot.get(&a), snapshot.get(&b)) {
                        let sim = crate::engine::vector::VectorLayer::best_similarity(
                            &ta.data.embedding, &ta.data.labels,
                            &tb.data.embedding, &tb.data.labels,
                        );
                        kg.add_relation(a, b,
                            crate::engine::knowledge::RelationType::SimilarTo,
                            (sim + boost).min(1.0));
                    }
                }
            }
            for &vid in &visited {
                mass_updates.push((vid, 0.002));
            }
        }

        for (id, delta) in &mass_updates {
            if let Err(e) = space.update_mass(*id, *delta) {
                tracing::debug!("[Pulse] mass update {} failed: {}", id, e);
            }
        }

        // 收集被修改 mass 的 tetra id，供调用方增量持久化
        let dirty_ids: Vec<TetraId> = mass_updates.iter().map(|(id, _)| *id).collect();

        let returned = port_vid.is_some();
        tracing::info!(
            "[Pulse] star-topology from tetra {} (port {:?}): visited {} tetras, returned={}, dirty={}",
            origin, port_vid, visited.len(), returned, dirty_ids.len()
        );

        let pulse = Pulse { id: 0, origin, ttl };
        let result = PulseResult {
            pulse_id: pulse.id,
            origin,
            reached_target: visited.len() > 1,
            data: crate::domain::pulse::PulseData {
                visited_tetras: visited,
                collected_content_hashes: collected_hashes,
                path_length: discoveries.len(),
                discoveries,
            },
            energy_cost: 0.5,
            dirty_ids,
        };

        Ok(result)
    }

    pub fn send_from_port(
        space: &Space,
        kg: &KnowledgeGraph,
        port_vid: VertexId,
        layer: CylinderLayer,
        max_hops: u32,
    ) -> Result<PulseReport, String> {
        let connected = space.tetras_connected_to_port(port_vid);
        if connected.is_empty() {
            return Err(format!("no tetrahedra connected to port {}", port_vid));
        }

        let origin = connected[0];
        let cluster = Self::bfs_cluster_from_port(space, port_vid, max_hops as usize);

        let mut report = PulseReport::new(port_vid, layer);
        report.returned = true;
        report.tetras_visited = cluster.len() + 1;

        let all = space.all_tetrahedrons();
        let snapshot: HashMap<TetraId, &crate::domain::tetra::Tetrahedron> =
            all.iter().map(|t| (t.id, t)).collect();

        for (tid, _hop) in &cluster {
            if let Some(t) = snapshot.get(tid) {
                report.data_collected.push(t.data.embedding.clone());
                report.content_hashes.push(t.data.content_hash);

                kg.add_relation(origin, *tid,
                    crate::engine::knowledge::RelationType::Related, 0.5);
            }
        }

        for (tid, _hop) in &cluster {
            let _ = space.update_mass(*tid, 0.02);
        }

        tracing::info!(
            "[Pulse] star-topology from port {} (layer {:?}): visited {} tetras, returned={}",
            port_vid, layer, report.tetras_visited, report.returned
        );

        Ok(report)
    }

    fn bfs_cluster_from_port(space: &Space, port_vid: VertexId, max_hops: usize) -> Vec<(TetraId, usize)> {
        let seeds = space.tetras_connected_to_port(port_vid);
        if seeds.is_empty() {
            return vec![];
        }

        let mut visited = HashSet::new();
        let mut queue: VecDeque<(TetraId, usize)> = VecDeque::new();
        let mut results = Vec::new();

        for &seed in &seeds {
            if visited.insert(seed) {
                queue.push_back((seed, 0));
            }
        }

        while let Some((current, dist)) = queue.pop_front() {
            if dist >= max_hops { continue; }
            for nid in space.neighbors_of(current) {
                if visited.insert(nid) {
                    results.push((nid, dist + 1));
                    queue.push_back((nid, dist + 1));
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    fn setup() -> (Space, KnowledgeGraph) {
        let space = Space::new();
        let kg = KnowledgeGraph::new();

        let topics = [
            ("Rust memory safety", vec!["rust".to_string()]),
            ("Python dynamic typing", vec!["python".to_string()]),
            ("Rust ownership model", vec!["rust".to_string()]),
            ("Python list comprehensions", vec!["python".to_string()]),
        ];
        for (i, (text, labels)) in topics.iter().enumerate() {
            let core = Point3::new(i as f64 * 1.0, 0.0, 0.0);
            let pos = Tetrahedron::compute_vertices(core);
            let t = Tetrahedron {
                id: 0, vertex_ids: [0; 4], core,
                data: MemoryPayload { content: text.to_string(), content_hash: i as u64, labels: labels.clone(), timestamp: 0, aliases: vec![], embedding: vec![], importance: 1.0, enforced: false, rationale: None, access_count: 0, memory_type: None, identity_stamp: None, source_agent: None, ..Default::default() },
                mass: 1.0,
            };
            space.add_tetrahedron(&t, &pos).unwrap();
        }

        (space, kg)
    }

    #[test]
    fn pulse_visits_origin() {
        let (space, kg) = setup();
        let r = PulseEngine::send(&space, &kg, PulseType::Neural { temperature: 0.8 }, 0, 5).unwrap();
        assert!(r.data.visited_tetras.len() >= 1, "pulse should visit at least origin");
    }

    #[test]
    fn star_topology_from_port() {
        let space = Space::new();
        let kg = KnowledgeGraph::new();

        let ports = space.cylinder_ports();
        assert!(!ports.is_empty());

        let (port_vid, port_pos) = ports[0];

        let offsets = Tetrahedron::compute_vertices(Point3::zero());
        let center = Point3::new(
            port_pos.x - offsets[0].x,
            port_pos.y - offsets[0].y,
            port_pos.z - offsets[0].z,
        );
        let verts = Tetrahedron::compute_vertices(center);
        let t = Tetrahedron {
            id: 0, vertex_ids: [0; 4], core: center,
            data: MemoryPayload {
                content: "star pulse test".into(), content_hash: 999, labels: vec!["test".into()],
                timestamp: 0, aliases: vec![], embedding: vec![0.5; 4], importance: 1.0,
                enforced: false, rationale: None, access_count: 0, memory_type: None,
                identity_stamp: None, source_agent: None,
            valid_from: 0, valid_to: None,
            expired_at: None, invalidated_at: None, memory_class: None,
            last_reviewed_ts: None,
            },
            mass: 1.0,
        };
        let tid = space.add_tetrahedron(&t, &verts).unwrap();

        let tet = space.get_tetrahedron(tid).unwrap();
        assert!(tet.vertex_ids.contains(&port_vid), "tetra should share port vertex");

        let report = PulseEngine::send_from_port(
            &space, &kg, port_vid, CylinderLayer::Instinct, 3,
        ).unwrap();

        assert!(report.returned, "pulse should return to port");
        assert!(report.tetras_visited >= 1, "should visit at least 1 tetra");
        assert_eq!(report.port_id, port_vid);
    }

    #[test]
    fn send_from_empty_port_fails() {
        let space = Space::new();
        let kg = KnowledgeGraph::new();
        let ports = space.cylinder_ports();
        let (port_vid, _) = ports[0];

        let result = PulseEngine::send_from_port(&space, &kg, port_vid, CylinderLayer::Instinct, 3);
        assert!(result.is_err(), "should fail when no tetra connected to port");
    }
}
