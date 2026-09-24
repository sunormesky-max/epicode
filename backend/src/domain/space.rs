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
    tetra_grid: HashMap<(i64, i64, i64), Vec<TetraId>>,
    next_vertex_id: VertexId,
    next_tetra_id: TetraId,
    cylinder: Cylinder,
    /// 结构版本：tetra 增删/relocate 递增，用于 cluster 缓存失效（避免重复 find_clusters O(N)）。
    structure_version: u64,
}

// ── Space ──

pub struct Space {
    inner: RwLock<SpaceInner>,
    /// Cluster 缓存：(structure_version, clusters)。find_clusters 内部按 version 失效。
    /// 放 Space 层使所有调用方（gateway.stats/scheduler/smrp）共享，根治高频 O(N) 重复聚类。
    cluster_cache: RwLock<Option<(u64, Vec<Cluster>)>>,
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
        let cylinder = Cylinder::new();
        let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
        let mut vertex_grid: HashMap<(i64, i64, i64), Vec<VertexId>> = HashMap::new();

        for port in cylinder.all_ports() {
            let vid = port.id;
            let pos = port.position;
            vertices.insert(vid, Vertex::new(vid, pos));
            let gk = grid_key(&pos);
            vertex_grid.entry(gk).or_default().push(vid);
        }

        let port_count = vertices.len();
        let max_port_vid = vertices.keys().max().copied().unwrap_or(0);

        tracing::info!(
            "[Space] registered {} cylinder port vertices (vid {}-{}) into spatial grid",
            port_count,
            1_000_000,
            max_port_vid
        );

        Self {
            inner: RwLock::new(SpaceInner {
                vertices,
                tetrahedrons: HashMap::new(),
                edge_table: HashMap::new(),
                face_table: HashMap::new(),
                vertex_to_tetras: HashMap::new(),
                vertex_grid,
                tetra_grid: HashMap::new(),
                next_vertex_id: 0,
                next_tetra_id: 0,
                cylinder,
                structure_version: 0,
            }),
            cluster_cache: RwLock::new(None),
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
        inner.structure_version += 1;
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
        inner.structure_version += 1;
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

        let core_for_grid = insert_tetra.core;
        inner.tetrahedrons.insert(id, insert_tetra);
        inner
            .tetra_grid
            .entry(grid_key(&core_for_grid))
            .or_default()
            .push(id);
        Ok(())
    }

    pub fn remove_tetrahedron(&self, id: TetraId) -> Result<Tetrahedron, String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .remove(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;

        let tgk = grid_key(&tetra.core);
        if let Some(ids) = inner.tetra_grid.get_mut(&tgk) {
            ids.retain(|&t| t != id);
            if ids.is_empty() {
                inner.tetra_grid.remove(&tgk);
            }
        }

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

        inner.structure_version += 1;
        Ok(tetra)
    }

    /// 结构版本号（tetra 增删/relocate 递增）——用于 cluster 缓存失效。
    pub fn structure_version(&self) -> u64 {
        self.inner.read().structure_version
    }

    pub fn get_tetrahedron(&self, id: TetraId) -> Option<Tetrahedron> {
        self.inner.read().tetrahedrons.get(&id).cloned()
    }

    pub fn update_mass(&self, id: TetraId, delta: f64) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.mass = (tetra.mass + delta).clamp(0.1, 100.0);
        // mass 不影响顶点共享拓扑,不递增 structure_version(B3修复)。
        // 之前这里递增导致 pulse 频繁更新 mass 时 find_clusters 反复全量重算 O(N)。
        Ok(())
    }

    pub fn update_aliases(&self, id: TetraId, aliases: Vec<String>) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
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
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data = payload;
        Ok(())
    }

    /// H5修复: 闭包式原子更新——单次写锁内完成 read+modify+write，消除 TOCTOU 竞态。
    /// 用于 Mem0 调和/A-MEM 进化等需要 read-modify-write 的场景。
    pub fn with_tetra_mut<F>(&self, id: TetraId, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut crate::domain::tetra::MemoryPayload) -> bool,
    {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        let changed = f(&mut tetra.data);
        if changed {
            inner.structure_version += 1;
        }
        Ok(())
    }

    /// M2修复:单字段更新 access_count,避免 get→clone(8KB embedding)→update_payload 的开销。
    pub fn update_access_count(&self, id: TetraId, count: u32) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.access_count = count;
        Ok(())
    }

    /// M2修复:单字段更新 importance。
    pub fn update_importance(&self, id: TetraId, importance: f64) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.importance = importance;
        Ok(())
    }

    /// 突破3: 更新最后复习时间（遗忘曲线 — 被访问的记忆重置衰减节拍）
    pub fn update_last_reviewed(&self, id: TetraId, ts: i64) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.last_reviewed_ts = Some(ts);
        Ok(())
    }

    pub fn update_labels(&self, id: TetraId, labels: Vec<String>) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.labels = labels;
        Ok(())
    }

    pub fn update_enforced(&self, id: TetraId, enforced: bool) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.enforced = enforced;
        Ok(())
    }

    pub fn update_validity(&self, id: TetraId, valid_to: Option<i64>) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.data.valid_to = valid_to;
        // H1 修复：双时序完整——设 valid_to 时同时记录系统得知失效的时间
        if valid_to.is_some() && tetra.data.invalidated_at.is_none() {
            tetra.data.invalidated_at = Some(chrono::Utc::now().timestamp());
        }
        Ok(())
    }

    pub fn update_vertex_ids(&self, id: TetraId, vertex_ids: [VertexId; 4]) -> Result<(), String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        tetra.vertex_ids = vertex_ids;
        Ok(())
    }

    pub fn tetra_count(&self) -> usize {
        self.inner.read().tetrahedrons.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.inner.read().vertices.len()
    }

    /// 骨架硬化：try 版 cluster count——写锁被持有时返回 None 而非阻塞
    /// 用于 SSE 等非关键路径，避免和 tick/dream 写操作竞争
    pub fn try_cluster_count(&self) -> Option<usize> {
        let cache = self.cluster_cache.try_read()?;
        cache.as_ref().map(|(_, clusters)| clusters.len())
    }

    pub fn non_port_vertex_count(&self) -> usize {
        let inner = self.inner.read();
        inner.vertices.values().filter(|v| v.id < 1_000_000).count()
    }

    pub fn cylinder_ports(&self) -> Vec<(VertexId, Point3)> {
        let inner = self.inner.read();
        inner
            .cylinder
            .all_ports()
            .iter()
            .map(|p| (p.id, p.position))
            .collect()
    }

    pub fn all_tetras_meta(&self) -> Vec<super::tetra::TetraMeta> {
        self.inner
            .read()
            .tetrahedrons
            .values()
            .map(|t| super::tetra::TetraMeta {
                id: t.id,
                core: t.core,
                mass: t.mass,
                content: t.data.content.chars().take(200).collect(),
                content_hash: t.data.content_hash,
                labels: t.data.labels.clone(),
                importance: t.data.importance,
                enforced: t.data.enforced,
                access_count: t.data.access_count,
                timestamp: t.data.timestamp,
            })
            .collect()
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

    /// SMRP §7.3 capacity — Port 占用统计：(assigned, free)。
    pub fn port_stats(&self) -> (usize, usize) {
        let inner = self.inner.read();
        let total = inner.cylinder.port_count();
        let mut free = 0;
        for layer in super::cylinder::CylinderLayer::all() {
            free += inner.cylinder.free_port_count(*layer);
        }
        (total.saturating_sub(free), free)
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

    /// 按 vid 精确指定 port 连接（kimi2.7 #1：替代 sentinel 匹配，防并发泄漏）
    pub fn assign_specific_port(&self, port_vid: VertexId, tetra_id: TetraId) -> bool {
        self.inner
            .write()
            .cylinder
            .assign_specific_port(port_vid, tetra_id)
            .is_ok()
    }

    /// 启动恢复：扫描所有 tetra 的 vertex_ids，重建 cylinder Port 占用状态。
    /// tetra.vertex_ids 持久化且 Port vid deterministic，故几何连接保留；
    /// 但 cylinder.ports 的 connected_tetra 占用状态在重启时丢失（cylinder 重建）。
    /// 此方法从持久化的 vertex_ids 反推哪些 Port 被哪个 tetra 占用，恢复一簇一Port 连接。
    pub fn rebuild_port_occupancy(&self) -> usize {
        let mut inner = self.inner.write();
        let port_vids: HashSet<VertexId> =
            inner.cylinder.all_ports().iter().map(|p| p.id).collect();
        let mut port_to_tetra: HashMap<VertexId, TetraId> = HashMap::new();
        for (&tid, tetra) in &inner.tetrahedrons {
            for &vid in &tetra.vertex_ids {
                if port_vids.contains(&vid) {
                    port_to_tetra.entry(vid).or_insert(tid);
                    break;
                }
            }
        }
        let restored = port_to_tetra.len();
        for (port_vid, tetra_id) in port_to_tetra {
            let _ = inner.cylinder.assign_specific_port(port_vid, tetra_id);
        }
        inner.structure_version += 1;
        tracing::info!(
            "[Space] rebuilt port occupancy: {} ports re-anchored",
            restored
        );
        restored
    }

    /// 深层突破1: Port reseed — 给没有 Port 的簇分配 Port。
    /// 遍历所有簇，对每个没有 Port 连接的簇，尝试分配一个空闲 Port。
    /// 这恢复了脉冲星型拓扑（圆柱→Port→簇），让 pulse/auto_pipeline 能从圆柱到达每个簇。
    pub fn reseed_ports(&self) -> usize {
        let mut inner = self.inner.write();

        // 1. 找出已有 Port 连接的 tetra（通过 cylinder 的 connected_tetra）
        let connected_tetras: HashSet<TetraId> = inner
            .cylinder
            .all_ports()
            .iter()
            .filter_map(|p| p.connected_tetra)
            .collect();

        // 2. 找出所有簇
        let tetra_verts: HashMap<TetraId, [VertexId; 4]> = inner
            .tetrahedrons
            .iter()
            .map(|(&id, t)| (id, t.vertex_ids))
            .collect();
        let v2t = inner.vertex_to_tetras.clone();

        let mut visited: HashSet<TetraId> = HashSet::new();
        let mut clusters: Vec<Vec<TetraId>> = Vec::new();
        for &id in tetra_verts.keys() {
            if visited.contains(&id) {
                continue;
            }
            let mut cluster_ids = Vec::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(id);
            visited.insert(id);
            while let Some(current) = queue.pop_front() {
                cluster_ids.push(current);
                if let Some(&vids) = tetra_verts.get(&current) {
                    for &vid in &vids {
                        if let Some(neighbors) = v2t.get(&vid) {
                            for &nid in neighbors {
                                if visited.insert(nid) {
                                    queue.push_back(nid);
                                }
                            }
                        }
                    }
                }
            }
            clusters.push(cluster_ids);
        }

        // 3. 对每个没有 Port 连接的簇，分配一个 Port
        let mut reseeded = 0usize;
        for cluster in &clusters {
            let has_port = cluster.iter().any(|id| connected_tetras.contains(id));
            if has_port {
                continue;
            }

            // 找该簇的语义层
            let first_tetra = match inner.tetrahedrons.get(&cluster[0]) {
                Some(t) => t,
                None => continue,
            };
            // 用第一个 tetra 的 core.z 判断层
            let layer = crate::domain::cylinder::CylinderLayer::from_index(
                (first_tetra.core.z / 2.0).round().max(0.0).min(5.0) as usize,
            )
            .unwrap_or(crate::domain::cylinder::CylinderLayer::Instinct);

            // 尝试分配 Port
            if let Some(port_vid) = inner.cylinder.assign_port(layer, cluster[0]) {
                let _ = inner.cylinder.assign_specific_port(port_vid, cluster[0]);
                reseeded += 1;
            }
        }

        inner.structure_version += 1;
        tracing::info!("[Space] port reseed: {} clusters got ports (of {} total clusters, {} already had ports)",
            reseeded, clusters.len(), clusters.len() - reseeded);
        reseeded
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

    pub fn cylinder_health_with_reports(
        &self,
        reports: &[super::cylinder::PulseReport],
    ) -> super::cylinder::HealthReport {
        self.inner.read().cylinder.health_check(reports)
    }

    pub fn port_vertex_of_tetra(&self, tetra_id: TetraId) -> Option<VertexId> {
        let inner = self.inner.read();
        let tetra = inner.tetrahedrons.get(&tetra_id)?;
        // 先查几何层（vertex_ids 含 Port vid）
        if let Some(&pvid) = tetra.vertex_ids.iter().find(|&&v| v >= 1_000_000) {
            return Some(pvid);
        }
        // 深层突破4: 再查逻辑层（cylinder 分配了 Port 但几何上未合并）
        // 返回该 tetra 被分配到的 Port vid
        for port in inner.cylinder.all_ports() {
            if port.connected_tetra == Some(tetra_id) {
                return Some(port.id);
            }
        }
        None
    }

    pub fn tetras_connected_to_port(&self, port_vid: VertexId) -> Vec<TetraId> {
        let inner = self.inner.read();
        inner
            .vertex_to_tetras
            .get(&port_vid)
            .cloned()
            .unwrap_or_default()
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
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;

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

        let old_tgk = grid_key(&removed.core);
        if let Some(ids) = inner.tetra_grid.get_mut(&old_tgk) {
            ids.retain(|&t| t != id);
            if ids.is_empty() {
                inner.tetra_grid.remove(&old_tgk);
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
        inner.structure_version += 1;
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
        // 缓存命中检查（按 structure_version 失效）——根治高频 O(N) 重复聚类
        let ver = {
            let inner = self.inner.read();
            inner.structure_version
        };
        {
            let cache = self.cluster_cache.read();
            if let Some((cv, ref clusters)) = *cache {
                if cv == ver {
                    return clusters.clone();
                }
            }
        }
        let (tetra_verts, v2t) = {
            let inner = self.inner.read();
            if inner.tetrahedrons.is_empty() {
                *self.cluster_cache.write() = Some((ver, vec![]));
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

        *self.cluster_cache.write() = Some((ver, clusters.clone()));
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

    // ── Nearest Neighbor (grid-accelerated, O(~100) typical) ──

    pub fn nearest_tetrahedron_to(&self, point: Point3) -> Option<(TetraId, f64)> {
        let inner = self.inner.read();

        let gk = grid_key(&point);
        let mut best: Option<(TetraId, f64)> = None;

        for nk in nearby_keys(gk) {
            if let Some(ids) = inner.tetra_grid.get(&nk) {
                for &id in ids {
                    if let Some(t) = inner.tetrahedrons.get(&id) {
                        let dist = t.core.distance_to(&point);
                        if best.is_none() || dist < best.as_ref().unwrap().1 {
                            best = Some((id, dist));
                        }
                    }
                }
            }
        }

        if best.is_some() {
            return best;
        }

        inner
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
        assert_eq!(space.non_port_vertex_count(), 4);
    }

    #[test]
    fn add_two_disjoint_tetras() {
        let space = Space::new();
        let (t1, p1) = make_tetra(0, Point3::new(0.0, 0.0, 0.0));
        let (t2, p2) = make_tetra(0, Point3::new(10.0, 0.0, 0.0));
        space.add_tetrahedron(&t1, &p1).unwrap();
        space.add_tetrahedron(&t2, &p2).unwrap();
        assert_eq!(space.tetra_count(), 2);
        assert_eq!(space.non_port_vertex_count(), 8);
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

        assert_eq!(space.non_port_vertex_count(), 7);
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
        assert_eq!(space.non_port_vertex_count(), 0);
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

    #[test]
    fn port_vertices_registered_in_space() {
        let space = Space::new();
        assert!(
            space.vertex_count() > 0,
            "space should have port vertices at init"
        );

        let ports: Vec<(VertexId, Point3)> = space.cylinder_ports();

        assert!(!ports.is_empty(), "should have port vertices");

        for (vid, pos) in &ports {
            assert!(*vid >= 1_000_000, "port vid should be >= 1M");
            let gk = grid_key(pos);
            let inner = space.inner.read();
            assert!(
                inner
                    .vertex_grid
                    .get(&gk)
                    .is_some_and(|vids| vids.contains(vid)),
                "port vid {} should be in vertex_grid",
                vid
            );
        }
    }

    #[test]
    fn tetra_vertex_merges_with_port() {
        let space = Space::new();

        let ports = space.cylinder_ports();
        let (port_vid, port_pos) = ports[0];

        let offsets = Tetrahedron::compute_vertices(Point3::zero());
        let center = Point3::new(
            port_pos.x - offsets[0].x,
            port_pos.y - offsets[0].y,
            port_pos.z - offsets[0].z,
        );

        let verts = Tetrahedron::compute_vertices(center);
        let merges = space.count_vertex_merges(&verts);
        assert!(
            merges >= 1,
            "at least 1 vertex should merge with port, got {}",
            merges
        );

        let (tetra, _) = make_tetra(0, center);
        let tid = space.add_tetrahedron(&tetra, &verts).unwrap();

        let tet = space.get_tetrahedron(tid).unwrap();
        assert!(
            tet.vertex_ids.contains(&port_vid),
            "tetra vertex_ids {:?} should contain port vid {}",
            tet.vertex_ids,
            port_vid
        );
    }

    #[test]
    fn six_zones_via_public_api() {
        let space = Space::new();
        use crate::domain::cylinder::CylinderLayer;
        let layers = [
            CylinderLayer::Instinct,
            CylinderLayer::Relation,
            CylinderLayer::Cognitive,
            CylinderLayer::Service,
            CylinderLayer::Cycle,
            CylinderLayer::Identity,
        ];
        for (i, layer) in layers.iter().enumerate() {
            let zone = space.zone_for_layer(*layer);
            let layer_height = 12.0 / 6.0;
            let expected_min = i as f64 * layer_height;
            assert!(
                (zone.z_min - expected_min).abs() < 1e-6,
                "layer {:?} z_min wrong",
                layer
            );
        }
    }
}
