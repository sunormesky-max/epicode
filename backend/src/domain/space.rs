use std::collections::{HashMap, HashSet, VecDeque};

use parking_lot::RwLock;

use super::cylinder::Cylinder;
use super::tetra::{TetraId, Tetrahedron};
use super::vertex::{Point3, Vertex, VertexId, VERTEX_MERGE_EPSILON};

// ── Edge & Face Tables ──

#[derive(Debug, Clone, Default)]
pub struct EdgeEntry {
    pub shared_by: Vec<TetraId>,
}

#[derive(Debug, Clone, Default)]
pub struct FaceEntry {
    pub shared_by: Vec<TetraId>,
}

// ── Cluster ──

#[derive(Debug, Clone)]
pub struct Cluster {
    pub tetra_ids: Vec<TetraId>,
}

// ── SpaceInner: all mutable state under a single RwLock ──

struct SpaceInner {
    vertices: HashMap<VertexId, Vertex>,
    tetrahedrons: HashMap<TetraId, Tetrahedron>,
    edge_table: HashMap<(VertexId, VertexId), EdgeEntry>,
    face_table: HashMap<[VertexId; 3], FaceEntry>,
    vertex_to_tetras: HashMap<VertexId, Vec<TetraId>>,
    vertex_grid: HashMap<(i64, i64, i64), Vec<VertexId>>,
    next_vertex_id: VertexId,
    next_tetra_id: TetraId,
    cylinder: Cylinder,
}

// ── Space ──

pub struct Space {
    inner: RwLock<SpaceInner>,
}

const GRID_CELL: f64 = 1.0;

fn grid_key(p: &Point3) -> (i64, i64, i64) {
    (
        (p.x / GRID_CELL).floor() as i64,
        (p.y / GRID_CELL).floor() as i64,
        (p.z / GRID_CELL).floor() as i64,
    )
}

fn nearby_keys(key: (i64, i64, i64)) -> Vec<(i64, i64, i64)> {
    let mut keys = Vec::with_capacity(27);
    for dx in -1i64..=1 {
        for dy in -1i64..=1 {
            for dz in -1i64..=1 {
                keys.push((key.0 + dx, key.1 + dy, key.2 + dz));
            }
        }
    }
    keys
}

impl Default for Space {
    fn default() -> Self {
        Self::new()
    }
}

impl Space {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(SpaceInner {
                vertices: HashMap::new(),
                tetrahedrons: HashMap::new(),
                edge_table: HashMap::new(),
                face_table: HashMap::new(),
                vertex_to_tetras: HashMap::new(),
                vertex_grid: HashMap::new(),
                next_vertex_id: 0,
                next_tetra_id: 0,
                cylinder: Cylinder::new(),
            }),
        }
    }

    // ── Tetrahedron CRUD ──

    pub fn add_tetrahedron(
        &self,
        tetra: &Tetrahedron,
        positions: &[Point3; 4],
    ) -> Result<TetraId, String> {
        if !Tetrahedron::validate_shape(positions) {
            return Err("tetrahedron is not regular".into());
        }
        let mut inner = self.inner.write();
        let id = inner.next_tetra_id;
        inner.next_tetra_id += 1;
        Self::insert_tetra(&mut inner, tetra, id, positions)?;
        Ok(id)
    }

    pub fn add_tetrahedron_with_id(
        &self,
        tetra: &Tetrahedron,
        positions: &[Point3; 4],
    ) -> Result<TetraId, String> {
        if !Tetrahedron::validate_shape(positions) {
            return Err("tetrahedron is not regular".into());
        }
        let mut inner = self.inner.write();
        if inner.tetrahedrons.contains_key(&tetra.id) {
            return Err(format!("tetrahedron {} already exists", tetra.id));
        }
        Self::insert_tetra(&mut inner, tetra, tetra.id, positions)?;
        if tetra.id >= inner.next_tetra_id {
            inner.next_tetra_id = tetra.id + 1;
        }
        Ok(tetra.id)
    }

    fn insert_tetra(
        inner: &mut SpaceInner,
        tetra: &Tetrahedron,
        id: TetraId,
        positions: &[Point3; 4],
    ) -> Result<(), String> {
        let mut vertex_ids = [0u64; 4];
        for i in 0..4 {
            let pos = &positions[i];
            let gk = grid_key(pos);
            let mut found: Option<VertexId> = None;
            for nk in nearby_keys(gk) {
                if let Some(vids) = inner.vertex_grid.get(&nk) {
                    for &vid in vids {
                        if let Some(v) = inner.vertices.get(&vid) {
                            if v.position.distance_to(pos) < VERTEX_MERGE_EPSILON {
                                found = Some(vid);
                                break;
                            }
                        }
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            let vid = match found {
                Some(vid) => {
                    tracing::debug!(
                        "[Space] vertex MERGE: tetra {} vertex[{}] merged into existing vid={} (dist={:.4})",
                        id, i, vid, inner.vertices.get(&vid).map(|v| v.position.distance_to(pos)).unwrap_or(0.0)
                    );
                    vid
                }
                None => {
                    let vid = inner.next_vertex_id;
                    inner.next_vertex_id += 1;
                    inner.vertices.insert(vid, Vertex::new(vid, *pos));
                    inner.vertex_grid.entry(gk).or_default().push(vid);
                    vid
                }
            };
            vertex_ids[i] = vid;
        }

        let merged_count = vertex_ids.iter().collect::<HashSet<_>>().len();
        if merged_count < 4 {
            tracing::info!(
                "[Space] tetra {} shares {} vertices with existing tetrahedra",
                id,
                4 - merged_count
            );
        }

        let mut insert_tetra = tetra.clone();
        insert_tetra.id = id;
        insert_tetra.vertex_ids = vertex_ids;

        for &vid in &vertex_ids {
            inner.vertex_to_tetras.entry(vid).or_default().push(id);
        }

        for &(i, j) in Tetrahedron::edges() {
            let key = ordered_pair(vertex_ids[i], vertex_ids[j]);
            inner.edge_table.entry(key).or_default().shared_by.push(id);
        }

        for &face_indices in Tetrahedron::faces() {
            let mut key = [
                vertex_ids[face_indices[0]],
                vertex_ids[face_indices[1]],
                vertex_ids[face_indices[2]],
            ];
            key.sort();
            inner.face_table.entry(key).or_default().shared_by.push(id);
        }

        inner.tetrahedrons.insert(id, insert_tetra);
        Ok(())
    }

    pub fn remove_tetrahedron(&self, id: TetraId) -> Result<Tetrahedron, String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .remove(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;

        for &vid in &tetra.vertex_ids {
            if let Some(ids) = inner.vertex_to_tetras.get_mut(&vid) {
                ids.retain(|&t| t != id);
                if ids.is_empty() {
                    inner.vertex_to_tetras.remove(&vid);
                    inner.vertices.remove(&vid);
                }
            }
        }

        for &(i, j) in Tetrahedron::edges() {
            let key = ordered_pair(tetra.vertex_ids[i], tetra.vertex_ids[j]);
            if let Some(entry) = inner.edge_table.get_mut(&key) {
                entry.shared_by.retain(|&t| t != id);
                if entry.shared_by.is_empty() {
                    inner.edge_table.remove(&key);
                }
            }
        }

        for &face_indices in Tetrahedron::faces() {
            let mut key = [
                tetra.vertex_ids[face_indices[0]],
                tetra.vertex_ids[face_indices[1]],
                tetra.vertex_ids[face_indices[2]],
            ];
            key.sort();
            if let Some(entry) = inner.face_table.get_mut(&key) {
                entry.shared_by.retain(|&t| t != id);
                if entry.shared_by.is_empty() {
                    inner.face_table.remove(&key);
                }
            }
        }

        inner.cylinder.release_port(id);

        Ok(tetra)
    }

    pub fn get_tetrahedron(&self, id: TetraId) -> Option<Tetrahedron> {
        self.inner.read().tetrahedrons.get(&id).cloned()
    }

    pub fn update_mass(&self, id: TetraId, delta: f64) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.mass = (tetra.mass + delta).clamp(0.1, 100.0);
        Ok(())
    }

    pub fn update_aliases(&self, id: TetraId, aliases: Vec<String>) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.data.aliases = aliases;
        Ok(())
    }

    pub fn update_payload(
        &self,
        id: TetraId,
        payload: crate::domain::tetra::MemoryPayload,
    ) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.data = payload;
        Ok(())
    }

    pub fn update_labels(&self, id: TetraId, labels: Vec<String>) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.data.labels = labels;
        Ok(())
    }

    pub fn update_enforced(&self, id: TetraId, enforced: bool) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.data.enforced = enforced;
        Ok(())
    }

    pub fn update_vertex_ids(&self, id: TetraId, vertex_ids: [VertexId; 4]) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;
        tetra.vertex_ids = vertex_ids;
        Ok(())
    }

    pub fn tetra_count(&self) -> usize {
        self.inner.read().tetrahedrons.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.inner.read().vertices.len()
    }

    pub fn edge_count(&self) -> usize {
        self.inner.read().edge_table.len()
    }

    pub fn max_tetra_id(&self) -> u64 {
        self.inner
            .read()
            .tetrahedrons
            .keys()
            .max()
            .copied()
            .unwrap_or(0)
    }

    pub fn max_vertex_id(&self) -> u64 {
        self.inner
            .read()
            .vertices
            .keys()
            .max()
            .copied()
            .unwrap_or(0)
    }

    pub fn restore_counters(&self) {
        let mut inner = self.inner.write();
        inner.next_tetra_id = inner.tetrahedrons.keys().max().copied().unwrap_or(0) + 1;
        inner.next_vertex_id = inner.vertices.keys().max().copied().unwrap_or(0) + 1;
    }

    pub fn all_tetrahedrons(&self) -> Vec<Tetrahedron> {
        self.inner.read().tetrahedrons.values().cloned().collect()
    }

    pub fn all_vertices(&self) -> Vec<Vertex> {
        self.inner.read().vertices.values().cloned().collect()
    }

    // ── Cylinder proxy methods (single lock) ──

    pub fn cylinder_radius(&self) -> f64 {
        self.inner.read().cylinder.radius()
    }

    pub fn cylinder_height(&self) -> f64 {
        self.inner.read().cylinder.height()
    }

    pub fn cylinder_port_count(&self) -> usize {
        self.inner.read().cylinder.port_count()
    }

    pub fn zone_for_layer(
        &self,
        layer: super::cylinder::CylinderLayer,
    ) -> super::cylinder::LayerZone {
        self.inner.read().cylinder.zone_for_layer(layer).clone()
    }

    pub fn assign_cylinder_port(
        &self,
        layer: super::cylinder::CylinderLayer,
        tetra_id: TetraId,
    ) -> Option<(VertexId, super::vertex::Point3)> {
        let mut inner = self.inner.write();
        let port_vid = inner.cylinder.assign_port(layer, tetra_id)?;
        let pos = inner
            .cylinder
            .port_position(port_vid)
            .unwrap_or(super::vertex::Point3::zero());
        Some((port_vid, pos))
    }

    pub fn release_cylinder_port(&self, tetra_id: TetraId) {
        self.inner.write().cylinder.release_port(tetra_id);
    }

    pub fn reassign_cylinder_port(&self, old_tetra_id: TetraId, new_tetra_id: TetraId) -> bool {
        self.inner
            .write()
            .cylinder
            .reassign_port(old_tetra_id, new_tetra_id)
    }

    pub fn free_port_count(&self, layer: super::cylinder::CylinderLayer) -> usize {
        self.inner.read().cylinder.free_port_count(layer)
    }

    pub fn is_identity_confirmed(&self) -> bool {
        self.inner.read().cylinder.is_identity_confirmed()
    }

    pub fn identity_info(&self) -> Option<super::cylinder::IdentityInfo> {
        self.inner.read().cylinder.identity().cloned()
    }

    pub fn cylinder_health(&self) -> super::cylinder::HealthReport {
        self.inner.read().cylinder.health_check(&[])
    }

    pub fn confirm_identity(
        &self,
        name: String,
        mission: String,
        author: String,
        extra: std::collections::HashMap<String, String>,
    ) {
        self.inner
            .write()
            .cylinder
            .confirm_identity(name, mission, author, extra);
    }

    pub fn update_identity(
        &self,
        name: Option<String>,
        mission: Option<String>,
        author: Option<String>,
        extra: Option<std::collections::HashMap<String, String>>,
    ) {
        self.inner
            .write()
            .cylinder
            .update_identity(name, mission, author, extra);
    }

    pub fn pending_identity(&self) -> super::cylinder::PendingIdentity {
        self.inner.read().cylinder.pending_identity.clone()
    }

    pub fn set_identity_step(&self, step: usize, value: String) {
        self.inner.write().cylinder.set_identity_step(step, value);
    }

    pub fn confirm_pending_identity(&self) -> bool {
        self.inner.write().cylinder.confirm_pending()
    }

    /// Return the IDs of all tetrahedra sharing at least one vertex with the given tetra.
    pub fn neighbors_of(&self, id: TetraId) -> Vec<TetraId> {
        let inner = self.inner.read();
        let tetra = match inner.tetrahedrons.get(&id) {
            Some(t) => t,
            None => return vec![],
        };
        let mut neighbors = HashSet::new();
        for &vid in &tetra.vertex_ids {
            if let Some(ids) = inner.vertex_to_tetras.get(&vid) {
                for &nid in ids {
                    if nid != id {
                        neighbors.insert(nid);
                    }
                }
            }
        }
        neighbors.into_iter().collect()
    }

    /// BFS to find tetrahedra reachable within `max_hops` vertex-sharing steps.
    /// Returns pairs of (tetra_id, hop_distance).
    pub fn bfs_neighbors(&self, origin: TetraId, max_hops: usize) -> Vec<(TetraId, usize)> {
        let inner = self.inner.read();
        let mut visited = HashSet::new();
        visited.insert(origin);
        let mut queue = VecDeque::new();
        queue.push_back((origin, 0usize));
        let mut results = Vec::new();

        while let Some((current, dist)) = queue.pop_front() {
            if dist >= max_hops {
                continue;
            }
            let tetra = match inner.tetrahedrons.get(&current) {
                Some(t) => t,
                None => continue,
            };
            for &vid in &tetra.vertex_ids {
                if let Some(ids) = inner.vertex_to_tetras.get(&vid) {
                    for &nid in ids {
                        if visited.insert(nid) {
                            results.push((nid, dist + 1));
                            queue.push_back((nid, dist + 1));
                        }
                    }
                }
            }
        }
        results
    }

    /// Return all tetra IDs that share a vertex with the given vertex.
    pub fn count_vertex_merges(&self, positions: &[Point3; 4]) -> i32 {
        let inner = self.inner.read();
        let mut count = 0i32;
        for pos in positions {
            let gk = grid_key(pos);
            for nk in nearby_keys(gk) {
                if let Some(vids) = inner.vertex_grid.get(&nk) {
                    for &vid in vids {
                        if let Some(v) = inner.vertices.get(&vid) {
                            if v.position.distance_to(pos) < VERTEX_MERGE_EPSILON {
                                count += 1;
                                break;
                            }
                        }
                    }
                }
            }
        }
        count
    }

    /// Atomically remove a tetrahedron and re-add it at a new position.
    /// All under one write lock — no TOCTOU window.
    pub fn relocate_tetrahedron(&self, id: TetraId, new_core: Point3) -> Result<TetraId, String> {
        let mut inner = self.inner.write();
        let removed = inner
            .tetrahedrons
            .remove(&id)
            .ok_or_else(|| format!("tetrahedron {id} not found"))?;

        for &vid in &removed.vertex_ids {
            if let Some(ids) = inner.vertex_to_tetras.get_mut(&vid) {
                ids.retain(|&t| t != id);
                if ids.is_empty() {
                    inner.vertex_to_tetras.remove(&vid);
                    inner.vertices.remove(&vid);
                }
            }
        }
        for &(i, j) in Tetrahedron::edges() {
            let key = ordered_pair(removed.vertex_ids[i], removed.vertex_ids[j]);
            if let Some(entry) = inner.edge_table.get_mut(&key) {
                entry.shared_by.retain(|&t| t != id);
                if entry.shared_by.is_empty() {
                    inner.edge_table.remove(&key);
                }
            }
        }
        for &face_indices in Tetrahedron::faces() {
            let mut key = [
                removed.vertex_ids[face_indices[0]],
                removed.vertex_ids[face_indices[1]],
                removed.vertex_ids[face_indices[2]],
            ];
            key.sort();
            if let Some(entry) = inner.face_table.get_mut(&key) {
                entry.shared_by.retain(|&t| t != id);
                if entry.shared_by.is_empty() {
                    inner.face_table.remove(&key);
                }
            }
        }

        let positions = Tetrahedron::compute_vertices(new_core);
        if !Tetrahedron::validate_shape(&positions) {
            inner.tetrahedrons.insert(id, removed);
            return Err("relocated tetrahedron is not regular".into());
        }

        let mut moved = removed;
        moved.core = new_core;
        moved.vertex_ids = [0; 4];
        Self::insert_tetra(&mut inner, &moved, id, &positions)?;
        Ok(id)
    }

    // ── Sharing Detection ──

    pub fn find_shared_vertices(&self, a: TetraId, b: TetraId) -> Vec<VertexId> {
        let inner = self.inner.read();
        let ta = match inner.tetrahedrons.get(&a) {
            Some(t) => t,
            None => return vec![],
        };
        let tb = match inner.tetrahedrons.get(&b) {
            Some(t) => t,
            None => return vec![],
        };
        ta.vertex_ids
            .iter()
            .filter(|vid| tb.vertex_ids.contains(vid))
            .copied()
            .collect()
    }

    // ── Clusters ──

    pub fn find_clusters(&self) -> Vec<Cluster> {
        let (tetra_verts, v2t) = {
            let inner = self.inner.read();
            if inner.tetrahedrons.is_empty() {
                return vec![];
            }
            let tv: HashMap<TetraId, [VertexId; 4]> = inner
                .tetrahedrons
                .iter()
                .map(|(&id, t)| (id, t.vertex_ids))
                .collect();
            (tv, inner.vertex_to_tetras.clone())
        };

        let mut visited: HashSet<TetraId> = HashSet::new();
        let mut clusters = Vec::new();

        for &id in tetra_verts.keys() {
            if visited.contains(&id) {
                continue;
            }

            let mut cluster_ids = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(id);
            visited.insert(id);

            while let Some(current) = queue.pop_front() {
                cluster_ids.push(current);
                if let Some(&vids) = tetra_verts.get(&current) {
                    for &vid in &vids {
                        if let Some(neighbors) = v2t.get(&vid) {
                            for &neighbor_id in neighbors {
                                if visited.insert(neighbor_id) {
                                    queue.push_back(neighbor_id);
                                }
                            }
                        }
                    }
                }
            }

            clusters.push(Cluster {
                tetra_ids: cluster_ids,
            });
        }

        for c in &mut clusters {
            c.tetra_ids.sort();
        }
        clusters.sort_by_key(|c| c.tetra_ids.first().copied().unwrap_or(0));

        clusters
    }

    // ── Edge Table Access ──

    pub fn edge_share_count(&self, v1: VertexId, v2: VertexId) -> usize {
        let key = ordered_pair(v1, v2);
        self.inner
            .read()
            .edge_table
            .get(&key)
            .map(|e| e.shared_by.len())
            .unwrap_or(0)
    }

    // ── Nearest Neighbor (naive, O(n)) ──

    pub fn nearest_tetrahedron_to(&self, point: Point3) -> Option<(TetraId, f64)> {
        self.inner
            .read()
            .tetrahedrons
            .iter()
            .map(|(id, t)| (*id, t.core.distance_to(&point)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }
}

/// Return a consistent ordered pair (min, max) for edge keys.
fn ordered_pair(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tetra::MemoryPayload;
    use super::*;

    fn make_tetra(id: TetraId, center: Point3) -> (Tetrahedron, [Point3; 4]) {
        let positions = Tetrahedron::compute_vertices(center);
        let tetra = Tetrahedron {
            id,
            vertex_ids: [0; 4],
            core: center,
            data: MemoryPayload::default(),
            mass: 1.0,
        };
        (tetra, positions)
    }

    #[test]
    fn add_single_tetra() {
        let space = Space::new();
        let (tetra, positions) = make_tetra(0, Point3::zero());
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert_eq!(id, 0);
        assert_eq!(space.tetra_count(), 1);
        assert_eq!(space.vertex_count(), 4);
    }

    #[test]
    fn add_two_disjoint_tetras() {
        let space = Space::new();
        let (t1, p1) = make_tetra(0, Point3::new(0.0, 0.0, 0.0));
        let (t2, p2) = make_tetra(0, Point3::new(10.0, 0.0, 0.0));
        space.add_tetrahedron(&t1, &p1).unwrap();
        space.add_tetrahedron(&t2, &p2).unwrap();
        assert_eq!(space.tetra_count(), 2);
        assert_eq!(space.vertex_count(), 8);
    }

    #[test]
    fn add_two_vertex_shared() {
        let space = Space::new();
        let c1 = Point3::zero();
        let c2 = Point3::new(1.0, 0.0, 0.0);
        let (t1, p1) = make_tetra(0, c1);
        let pos2 = Tetrahedron::compute_vertices(c2);
        let mut p2 = pos2;
        p2[1] = p1[0];
        let (t2, _) = make_tetra(0, c2);

        space.add_tetrahedron(&t1, &p1).unwrap();
        space.add_tetrahedron(&t2, &p2).unwrap();

        assert_eq!(space.vertex_count(), 7);
        assert_eq!(space.tetra_count(), 2);
    }

    #[test]
    fn reject_non_regular() {
        let space = Space::new();
        let mut positions = Tetrahedron::compute_vertices(Point3::zero());
        positions[0].x += 10.0;
        let tetra = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core: Point3::zero(),
            data: MemoryPayload::default(),
            mass: 1.0,
        };
        assert!(space.add_tetrahedron(&tetra, &positions).is_err());
    }

    #[test]
    fn shared_vertices_detection() {
        let space = Space::new();
        let c1 = Point3::new(0.0, 0.0, 0.0);
        let c2 = Point3::new(-1.0, 0.0, 0.0);
        let (t1, p1) = make_tetra(0, c1);
        let p2 = Tetrahedron::compute_vertices(c2);
        let (t2, _) = make_tetra(0, c2);

        let a = space.add_tetrahedron(&t1, &p1).unwrap();
        let b = space.add_tetrahedron(&t2, &p2).unwrap();

        let shared = space.find_shared_vertices(a, b);
        assert_eq!(shared.len(), 1);
    }

    #[test]
    fn find_clusters_disjoint() {
        let space = Space::new();
        let (t1, p1) = make_tetra(0, Point3::zero());
        let (t2, p2) = make_tetra(0, Point3::new(10.0, 0.0, 0.0));
        space.add_tetrahedron(&t1, &p1).unwrap();
        space.add_tetrahedron(&t2, &p2).unwrap();
        let clusters = space.find_clusters();
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn find_clusters_connected() {
        let space = Space::new();
        let c1 = Point3::new(0.0, 0.0, 0.0);
        let c2 = Point3::new(-1.0, 0.0, 0.0);
        let (t1, p1) = make_tetra(0, c1);
        let p2 = Tetrahedron::compute_vertices(c2);
        let (t2, _) = make_tetra(0, c2);

        let _a = space.add_tetrahedron(&t1, &p1).unwrap();
        let _b = space.add_tetrahedron(&t2, &p2).unwrap();
        let clusters = space.find_clusters();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].tetra_ids.len(), 2);
    }

    #[test]
    fn edge_share_count() {
        let space = Space::new();
        let c = Point3::zero();
        let (t1, p1) = make_tetra(0, c);
        let (t2, p2) = make_tetra(0, c);

        space.add_tetrahedron(&t1, &p1).unwrap();
        space.add_tetrahedron(&t2, &p2).unwrap();

        let v_ids = {
            let inner = space.inner.read();
            let t = inner.tetrahedrons.values().next().unwrap();
            t.vertex_ids
        };
        let count = space.edge_share_count(v_ids[0], v_ids[1]);
        assert_eq!(count, 2);
    }

    #[test]
    fn remove_tetra_cleans_up() {
        let space = Space::new();
        let (t, p) = make_tetra(0, Point3::zero());
        let id = space.add_tetrahedron(&t, &p).unwrap();
        assert_eq!(space.tetra_count(), 1);

        let removed = space.remove_tetrahedron(id).unwrap();
        assert_eq!(removed.id, id);
        assert_eq!(space.tetra_count(), 0);
        assert_eq!(space.vertex_count(), 0);
    }

    #[test]
    fn nearest_tetra_found() {
        let space = Space::new();
        let (t, p) = make_tetra(0, Point3::new(5.0, 0.0, 0.0));
        space.add_tetrahedron(&t, &p).unwrap();
        let result = space.nearest_tetrahedron_to(Point3::new(4.9, 0.0, 0.0));
        assert!(result.is_some());
        let (_found_id, dist) = result.unwrap();
        assert!(dist < 0.2);
    }

    #[test]
    fn remove_readd_preserves_cluster() {
        let space = Space::new();
        let c1 = Point3::new(0.0, 0.0, 0.0);
        let c2 = Point3::new(1.0, 0.0, 0.0);
        let (t1, p1) = make_tetra(0, c1);
        let p2 = Tetrahedron::compute_vertices(c2);
        let (t2, _) = make_tetra(0, c2);

        let _a = space.add_tetrahedron(&t1, &p1).unwrap();
        let b = space.add_tetrahedron(&t2, &p2).unwrap();

        assert_eq!(
            space.find_clusters().len(),
            1,
            "should be 1 cluster before remove+readd"
        );

        let vertices_before = space.vertex_count();

        let removed = space.remove_tetrahedron(b).unwrap();
        let positions = Tetrahedron::compute_vertices(removed.core);
        let mut moved = removed;
        moved.vertex_ids = [0; 4];
        let _new_b = space.add_tetrahedron(&moved, &positions).unwrap();

        let clusters_after = space.find_clusters();
        assert_eq!(
            clusters_after.len(),
            1,
            "should still be 1 cluster after remove+readd at same position, got {}",
            clusters_after.len()
        );
        assert_eq!(
            space.vertex_count(),
            vertices_before,
            "vertex count should be preserved"
        );
    }

    #[test]
    fn repeated_remove_readd_preserves_large_cluster() {
        let space = Space::new();
        let mut ids = Vec::new();
        for i in 0..10 {
            let core = Point3::new(i as f64, 0.0, 0.0);
            let (t, p) = make_tetra(0, core);
            let id = space.add_tetrahedron(&t, &p).unwrap();
            ids.push(id);
        }

        assert_eq!(
            space.find_clusters().len(),
            1,
            "10 tetras in a chain should be 1 cluster"
        );

        let vertices_before = space.vertex_count();

        for _ in 0..5 {
            let mut new_ids = Vec::new();
            for &id in &ids {
                let removed = space.remove_tetrahedron(id).unwrap();
                let positions = Tetrahedron::compute_vertices(removed.core);
                let mut moved = removed;
                moved.vertex_ids = [0; 4];
                let new_id = space.add_tetrahedron(&moved, &positions).unwrap();
                new_ids.push(new_id);
            }
            ids = new_ids;

            let clusters = space.find_clusters();
            assert_eq!(
                clusters.len(),
                1,
                "should still be 1 cluster after remove+readd round, got {}",
                clusters.len()
            );
        }

        assert_eq!(
            space.vertex_count(),
            vertices_before,
            "vertex count should be preserved"
        );
    }

    #[test]
    fn nearest_tetra_empty() {
        let space = Space::new();
        assert!(space.nearest_tetrahedron_to(Point3::zero()).is_none());
    }
}
