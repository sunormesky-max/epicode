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
    /// 搜索语料版本：增删及 content/alias 变更递增；通用 payload 更新保守递增。
    search_revision: u64,
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
                search_revision: 0,
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
        let next_tetra_id = id
            .checked_add(1)
            .ok_or_else(|| "tetrahedron ID space exhausted".to_string())?;
        Self::insert_tetra(&mut inner, tetra, id, positions)?;
        inner.next_tetra_id = next_tetra_id;
        inner.structure_version += 1;
        inner.search_revision += 1;
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
        let next_tetra_id = if tetra.id >= inner.next_tetra_id {
            Some(
                tetra
                    .id
                    .checked_add(1)
                    .ok_or_else(|| "tetrahedron ID space exhausted".to_string())?,
            )
        } else {
            None
        };
        Self::insert_tetra(&mut inner, tetra, tetra.id, positions)?;
        if let Some(next_tetra_id) = next_tetra_id {
            inner.next_tetra_id = next_tetra_id;
        }
        inner.structure_version += 1;
        inner.search_revision += 1;
        Ok(tetra.id)
    }

    fn insert_tetra(
        inner: &mut SpaceInner,
        tetra: &Tetrahedron,
        id: TetraId,
        positions: &[Point3; 4],
    ) -> Result<(), String> {
        let mut vertex_ids = [0u64; 4];
        // Stage new vertices until all IDs have been allocated. A counter near
        // exhaustion must not leave a partially inserted tetrahedron behind.
        // Shape validation ensures the four positions cannot merge with each other.
        let mut new_vertices = Vec::with_capacity(4);
        let mut next_vertex_id = inner.next_vertex_id;
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
                    // Cylinder Port IDs and ordinary vertex IDs share a u64
                    // namespace. Skip every occupied ID, including real Ports.
                    let vid = Self::next_free_vertex_id(&inner.vertices, &mut next_vertex_id)?;
                    new_vertices.push((vid, *pos, gk));
                    vid
                }
            };
            vertex_ids[i] = vid;
        }

        inner.next_vertex_id = next_vertex_id;
        for (vid, pos, gk) in new_vertices {
            inner.vertices.insert(vid, Vertex::new(vid, pos));
            inner.vertex_grid.entry(gk).or_default().push(vid);
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

    fn next_free_vertex_id(
        vertices: &HashMap<VertexId, Vertex>,
        next_vertex_id: &mut VertexId,
    ) -> Result<VertexId, String> {
        while vertices.contains_key(next_vertex_id) {
            *next_vertex_id = next_vertex_id
                .checked_add(1)
                .ok_or_else(|| "vertex ID space exhausted".to_string())?;
        }
        let id = *next_vertex_id;
        *next_vertex_id = id
            .checked_add(1)
            .ok_or_else(|| "vertex ID space exhausted".to_string())?;
        Ok(id)
    }

    /// Relocation removes the old geometry before inserting the new one. Check
    /// the worst case of four new vertices first so ID exhaustion cannot erase
    /// the old tetrahedron. A removal can only make more IDs available.
    fn ensure_vertex_id_capacity(inner: &SpaceInner, count: usize) -> Result<(), String> {
        let mut next_vertex_id = inner.next_vertex_id;
        for _ in 0..count {
            Self::next_free_vertex_id(&inner.vertices, &mut next_vertex_id)?;
        }
        Ok(())
    }

    pub fn remove_tetrahedron(&self, id: TetraId) -> Result<Tetrahedron, String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .remove(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;

        Self::remove_tetra_indexes(&mut inner, &tetra);
        Self::release_or_reanchor_port(&mut inner, id);

        inner.structure_version += 1;
        inner.search_revision += 1;
        Ok(tetra)
    }

    /// Drop a tetrahedron's index entries while retaining Cylinder-owned Port vertices.
    /// Both delete and relocation must remove ordinary orphan vertices from the spatial grid.
    fn remove_tetra_indexes(inner: &mut SpaceInner, tetra: &Tetrahedron) {
        let id = tetra.id;
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
                    if !inner.cylinder.all_ports().iter().any(|p| p.id == vid) {
                        if let Some(vertex) = inner.vertices.remove(&vid) {
                            let vgk = grid_key(&vertex.position);
                            if let Some(grid_ids) = inner.vertex_grid.get_mut(&vgk) {
                                grid_ids.retain(|&v| v != vid);
                                if grid_ids.is_empty() {
                                    inner.vertex_grid.remove(&vgk);
                                }
                            }
                        }
                    }
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
    }

    /// Preserve a Port's logical anchor when another tetrahedron still touches it.
    fn release_or_reanchor_port(inner: &mut SpaceInner, tetra_id: TetraId) {
        let Some(port_vid) = inner.cylinder.find_port_for_tetra(tetra_id).map(|p| p.id) else {
            return;
        };
        inner.cylinder.release_port(tetra_id);
        let replacement = inner.vertex_to_tetras.get(&port_vid).and_then(|ids| {
            ids.iter()
                .copied()
                .find(|&id| inner.cylinder.find_port_for_tetra(id).is_none())
        });
        if let Some(id) = replacement {
            let _ = inner.cylinder.assign_specific_port(port_vid, id);
        }
    }

    /// 结构版本号（tetra 增删/relocate 递增）——用于 cluster 缓存失效。
    pub fn structure_version(&self) -> u64 {
        self.inner.read().structure_version
    }

    /// Searchable-memory version, independent from the topology/cluster version.
    pub fn search_revision(&self) -> u64 {
        self.inner.read().search_revision
    }

    /// Clone the corpus and its version under one read lock for cache rebuilds.
    pub fn all_tetrahedrons_with_search_revision(&self) -> (u64, Vec<Tetrahedron>) {
        let inner = self.inner.read();
        (
            inner.search_revision,
            inner.tetrahedrons.values().cloned().collect(),
        )
    }

    pub fn get_tetrahedron(&self, id: TetraId) -> Option<Tetrahedron> {
        self.inner.read().tetrahedrons.get(&id).cloned()
    }

    /// Read a candidate set only if the searchable corpus still has this revision.
    pub fn get_tetrahedrons_by_ids_at_search_revision(
        &self,
        ids: &std::collections::HashSet<TetraId>,
        search_revision: u64,
    ) -> Option<Vec<Tetrahedron>> {
        let inner = self.inner.read();
        if inner.search_revision != search_revision {
            return None;
        }
        Some(
            ids.iter()
                .filter_map(|id| inner.tetrahedrons.get(id).cloned())
                .collect(),
        )
    }

    /// Clone only the bounded fallback window if the corpus revision still matches.
    pub fn first_tetrahedrons_at_search_revision(
        &self,
        limit: usize,
        search_revision: u64,
    ) -> Option<Vec<Tetrahedron>> {
        let inner = self.inner.read();
        if inner.search_revision != search_revision {
            return None;
        }
        Some(inner.tetrahedrons.values().take(limit).cloned().collect())
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
        let changed = {
            let tetra = inner
                .tetrahedrons
                .get_mut(&id)
                .ok_or_else(|| format!("tetrahedron {} not found", id))?;
            if tetra.data.aliases == aliases {
                false
            } else {
                tetra.data.aliases = aliases;
                true
            }
        };
        if changed {
            inner.search_revision += 1;
        }
        Ok(())
    }

    pub fn update_payload(
        &self,
        id: TetraId,
        payload: crate::domain::tetra::MemoryPayload,
    ) -> Result<(), String> {
        let mut inner = self.inner.write();
        let searchable_changed = {
            let tetra = inner
                .tetrahedrons
                .get_mut(&id)
                .ok_or_else(|| format!("tetrahedron {} not found", id))?;
            let changed =
                tetra.data.content != payload.content || tetra.data.aliases != payload.aliases;
            tetra.data = payload;
            changed
        };
        if searchable_changed {
            inner.search_revision += 1;
        }
        Ok(())
    }

    /// 在单次写锁内原子应用一组字段级操作,返回是否有任一操作改变了状态。
    /// 只触碰操作声明的字段,不会覆盖并发写入的其他字段(见 domain::ops)。
    pub fn apply_ops(&self, id: TetraId, ops: &[super::ops::MemoryOp]) -> Result<bool, String> {
        let mut inner = self.inner.write();
        let tetra = inner
            .tetrahedrons
            .get_mut(&id)
            .ok_or_else(|| format!("tetrahedron {} not found", id))?;
        let labels_before = tetra.data.labels.len();
        let mut changed = false;
        for op in ops {
            changed |= super::ops::apply_op(&mut tetra.data, &mut tetra.mass, op);
        }
        // 标签变化与 with_tetra_mut 保持一致:失效聚类/检索缓存
        if tetra.data.labels.len() != labels_before {
            inner.structure_version += 1;
            inner.search_revision += 1;
        }
        Ok(changed)
    }

    /// H5修复: 闭包式原子更新——单次写锁内完成 read+modify+write，消除 TOCTOU 竞态。
    /// 用于 Mem0 调和/A-MEM 进化等需要 read-modify-write 的场景。
    pub fn with_tetra_mut<F>(&self, id: TetraId, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut crate::domain::tetra::MemoryPayload) -> bool,
    {
        let mut inner = self.inner.write();
        let changed = {
            let tetra = inner
                .tetrahedrons
                .get_mut(&id)
                .ok_or_else(|| format!("tetrahedron {} not found", id))?;
            f(&mut tetra.data)
        };
        if changed {
            inner.structure_version += 1;
            inner.search_revision += 1;
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
        let port_ids: HashSet<VertexId> = inner.cylinder.all_ports().iter().map(|p| p.id).collect();
        inner
            .vertices
            .keys()
            .filter(|&&id| !port_ids.contains(&id))
            .count()
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
        inner.next_tetra_id = inner
            .tetrahedrons
            .keys()
            .max()
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        let port_ids: HashSet<VertexId> = inner.cylinder.all_ports().iter().map(|p| p.id).collect();
        inner.next_vertex_id = inner
            .vertices
            .keys()
            .filter(|&&id| !port_ids.contains(&id))
            .max()
            .map_or(0, |&id| id.saturating_add(1));
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
        let port_vids: HashSet<VertexId> = inner
            .cylinder
            .all_ports()
            .iter()
            .map(|port| port.id)
            .collect();

        let mut visited: HashSet<TetraId> = HashSet::new();
        let mut clusters: Vec<Vec<TetraId>> = Vec::new();
        // HashMap iteration order changes across restarts. Visit the oldest
        // tetrahedron in each cluster first so virtual Port links are stable.
        let mut tetra_ids: Vec<TetraId> = tetra_verts.keys().copied().collect();
        tetra_ids.sort_unstable();
        for id in tetra_ids {
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

            // Prefer a real shared vertex if direct Space insertion left its
            // Port unclaimed. A virtual edge is only a fallback for a truly
            // distant cluster.
            let mut contacts: Vec<(VertexId, TetraId)> = cluster
                .iter()
                .flat_map(|&id| {
                    tetra_verts
                        .get(&id)
                        .into_iter()
                        .flatten()
                        .filter(|vid| port_vids.contains(vid))
                        .map(move |&vid| (vid, id))
                })
                .collect();
            contacts.sort_unstable();
            if contacts
                .iter()
                .any(|&(port_vid, id)| inner.cylinder.assign_specific_port(port_vid, id).is_ok())
            {
                reseeded += 1;
                continue;
            }

            // 找该簇的语义层
            let first_tetra = match inner.tetrahedrons.get(&cluster[0]) {
                Some(t) => t,
                None => continue,
            };
            // The zone owns the layer boundary. Rounding z/2 assigned center-z
            // memories (1, 3, 5, ...) to the next layer.
            let Some(layer) = crate::domain::cylinder::CylinderLayer::all()
                .iter()
                .copied()
                .find(|&layer| {
                    inner
                        .cylinder
                        .zone_for_layer(layer)
                        .contains_z(first_tetra.core.z)
                })
            else {
                continue;
            };
            if !layer.has_ports() {
                continue;
            }

            // 尝试分配 Port
            if inner.cylinder.assign_port(layer, cluster[0]).is_some() {
                reseeded += 1;
            }
        }

        if reseeded > 0 {
            inner.structure_version += 1;
        }
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
        // 先查几何层：只有 Cylinder 实际持有的 ID 才是 Port。
        if let Some(&pvid) = tetra
            .vertex_ids
            .iter()
            .find(|&&vid| inner.cylinder.all_ports().iter().any(|p| p.id == vid))
        {
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

    /// Find the nearest Port among tetrahedra visited by a pulse. Distance is
    /// measured from the pulse origin; ties are resolved by Port ID.
    pub fn nearest_port_in_tetras(&self, reached: &[(TetraId, usize)]) -> Option<VertexId> {
        let inner = self.inner.read();
        let distances: HashMap<TetraId, usize> = reached.iter().copied().collect();
        let mut nearest: Option<(usize, VertexId)> = None;
        for port in inner.cylinder.all_ports() {
            let mut consider = |id: TetraId| {
                if let Some(&distance) = distances.get(&id) {
                    let candidate = (distance, port.id);
                    if nearest.is_none_or(|current| candidate < current) {
                        nearest = Some(candidate);
                    }
                }
            };
            if port.status == super::cylinder::PortStatus::Occupied {
                if let Some(owner) = port.connected_tetra {
                    consider(owner);
                }
            }
            if let Some(ids) = inner.vertex_to_tetras.get(&port.id) {
                for &id in ids {
                    consider(id);
                }
            }
        }
        nearest.map(|(_, port_id)| port_id)
    }

    pub fn tetras_connected_to_port(&self, port_vid: VertexId) -> Vec<TetraId> {
        let inner = self.inner.read();
        let Some(port) = inner
            .cylinder
            .all_ports()
            .iter()
            .find(|port| port.id == port_vid)
        else {
            return Vec::new();
        };
        let mut connected = inner
            .vertex_to_tetras
            .get(&port_vid)
            .cloned()
            .unwrap_or_default();
        // Reseeding creates a deliberate logical edge when an existing cluster
        // is too far from the Cylinder for a geometric shared vertex. Pulses
        // must enter through that edge as well as through geometric contacts.
        if port.status == super::cylinder::PortStatus::Occupied {
            if let Some(owner) = port
                .connected_tetra
                .filter(|id| inner.tetrahedrons.contains_key(id))
            {
                connected.push(owner);
            }
        }
        connected.sort_unstable();
        connected.dedup();
        connected
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
        if !inner.tetrahedrons.contains_key(&id) {
            return Err(format!("tetrahedron {} not found", id));
        }
        let positions = Tetrahedron::compute_vertices(new_core);
        if !new_core.x.is_finite()
            || !new_core.y.is_finite()
            || !new_core.z.is_finite()
            || !Tetrahedron::validate_shape(&positions)
        {
            return Err("relocated tetrahedron is not regular".into());
        }
        Self::ensure_vertex_id_capacity(&inner, 4)?;

        let old_port = inner.cylinder.find_port_for_tetra(id).map(|port| port.id);
        let old_port_was_geometric = old_port.is_some_and(|port_vid| {
            inner
                .tetrahedrons
                .get(&id)
                .is_some_and(|tetra| tetra.vertex_ids.contains(&port_vid))
        });
        let removed = inner
            .tetrahedrons
            .remove(&id)
            .expect("tetrahedron exists under the write lock");
        Self::remove_tetra_indexes(&mut inner, &removed);

        let mut moved = removed;
        moved.core = new_core;
        moved.vertex_ids = [0; 4];
        Self::insert_tetra(&mut inner, &moved, id, &positions)?;

        // A moved anchor must not keep reserving a Port it no longer touches.
        let new_ports: Vec<VertexId> = inner
            .tetrahedrons
            .get(&id)
            .expect("relocated tetrahedron was inserted")
            .vertex_ids
            .iter()
            .copied()
            .filter(|&vid| inner.cylinder.all_ports().iter().any(|p| p.id == vid))
            .collect();
        // Reseeding can make a logical-only Port connection. Keep that assignment when
        // the tetrahedron moves without touching any Port; only a geometric departure
        // or arrival at another Port changes the anchor.
        if old_port.is_some_and(|vid| {
            !new_ports.contains(&vid) && (old_port_was_geometric || !new_ports.is_empty())
        }) {
            Self::release_or_reanchor_port(&mut inner, id);
        }
        if inner.cylinder.find_port_for_tetra(id).is_none() {
            for port_vid in new_ports {
                if inner.cylinder.assign_specific_port(port_vid, id).is_ok() {
                    break;
                }
            }
        }

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

    fn center_with_vertex_at(position: Point3) -> Point3 {
        let offset = Tetrahedron::compute_vertices(Point3::zero())[0];
        Point3::new(
            position.x - offset.x,
            position.y - offset.y,
            position.z - offset.z,
        )
    }

    fn assert_vertex_grid_consistent(space: &Space) {
        let inner = space.inner.read();
        for (cell, ids) in &inner.vertex_grid {
            assert!(!ids.is_empty(), "empty vertex grid cell: {:?}", cell);
            for id in ids {
                let vertex = inner
                    .vertices
                    .get(id)
                    .expect("grid references missing vertex");
                assert_eq!(*cell, grid_key(&vertex.position));
            }
        }
        for (id, vertex) in &inner.vertices {
            assert!(
                inner
                    .vertex_grid
                    .get(&grid_key(&vertex.position))
                    .is_some_and(|ids| ids.contains(id)),
                "vertex {} is absent from its spatial grid cell",
                id
            );
        }
    }

    #[test]
    fn search_revision_tracks_searchable_memory_changes() {
        let space = Space::new();
        let (mut tetra, positions) = make_tetra(0, Point3::zero());
        tetra.data.content = "alpha".to_string();
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        let topology_revision = space.structure_version();
        assert_eq!(space.search_revision(), 1);

        let mut payload = space.get_tetrahedron(id).unwrap().data;
        payload.content = "beta".to_string();
        space.update_payload(id, payload).unwrap();
        assert_eq!(space.search_revision(), 2);
        assert_eq!(space.structure_version(), topology_revision);

        let mut payload = space.get_tetrahedron(id).unwrap().data;
        payload.labels.push("metadata-only".to_string());
        space.update_payload(id, payload).unwrap();
        assert_eq!(space.search_revision(), 2);

        let aliases = vec!["synonym".to_string()];
        space.update_aliases(id, aliases.clone()).unwrap();
        assert_eq!(space.search_revision(), 3);
        space.update_aliases(id, aliases).unwrap();
        assert_eq!(space.search_revision(), 3);

        space
            .with_tetra_mut(id, |payload| {
                payload.content = "gamma".to_string();
                true
            })
            .unwrap();
        assert_eq!(space.search_revision(), 4);

        space.remove_tetrahedron(id).unwrap();
        assert_eq!(space.search_revision(), 5);
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
    fn ordinary_ids_skip_ports_and_only_real_port_vertices_are_classified_as_ports() {
        let space = Space::new();
        let ports = space.cylinder_ports();
        let first_port_id = ports[0].0;
        let last_port_id = ports.last().unwrap().0;
        space.inner.write().next_vertex_id = first_port_id - 1;

        let (ordinary, positions) = make_tetra(0, Point3::new(20.0, 20.0, 20.0));
        let ordinary_id = space.add_tetrahedron(&ordinary, &positions).unwrap();
        let ordinary_ids = space.get_tetrahedron(ordinary_id).unwrap().vertex_ids;
        assert_eq!(ordinary_ids[0], first_port_id - 1);
        assert!(ordinary_ids[1..].iter().all(|&vid| vid > last_port_id));
        assert_eq!(space.port_vertex_of_tetra(ordinary_id), None);
        assert_eq!(space.non_port_vertex_count(), 4);

        let (port_id, port_pos) = ports[0];
        let (anchored, anchored_positions) = make_tetra(0, center_with_vertex_at(port_pos));
        let anchored_id = space
            .add_tetrahedron(&anchored, &anchored_positions)
            .unwrap();
        assert!(space
            .get_tetrahedron(anchored_id)
            .unwrap()
            .vertex_ids
            .contains(&port_id));
        assert_eq!(space.port_vertex_of_tetra(anchored_id), Some(port_id));
        assert_eq!(space.non_port_vertex_count(), 7);
        let inner = space.inner.read();
        for (id, position) in ports {
            assert_eq!(inner.vertices.get(&id).unwrap().position, position);
        }
        drop(inner);
        assert_vertex_grid_consistent(&space);
    }

    #[test]
    fn restore_counter_tracks_high_ordinary_ids_without_including_ports() {
        let space = Space::new();
        space.restore_counters();
        assert_eq!(space.inner.read().next_vertex_id, 0);

        let last_port_id = space.cylinder_ports().last().unwrap().0;
        space.inner.write().next_vertex_id = last_port_id + 1;
        let (first, first_positions) = make_tetra(0, Point3::new(20.0, 20.0, 20.0));
        let first_id = space.add_tetrahedron(&first, &first_positions).unwrap();
        assert_eq!(space.port_vertex_of_tetra(first_id), None);
        let last_ordinary_id = *space
            .get_tetrahedron(first_id)
            .unwrap()
            .vertex_ids
            .last()
            .unwrap();

        space.restore_counters();
        assert_eq!(space.inner.read().next_vertex_id, last_ordinary_id + 1);
        let (second, second_positions) = make_tetra(0, Point3::new(30.0, 20.0, 20.0));
        let second_id = space.add_tetrahedron(&second, &second_positions).unwrap();
        assert!(space
            .get_tetrahedron(second_id)
            .unwrap()
            .vertex_ids
            .iter()
            .all(|&vid| vid > last_ordinary_id));
        assert_eq!(space.port_vertex_of_tetra(second_id), None);
        assert_eq!(space.non_port_vertex_count(), 8);
    }

    #[test]
    fn exhausted_vertex_id_does_not_partially_insert_vertices() {
        let space = Space::new();
        space.inner.write().next_vertex_id = u64::MAX;
        let port_count = space.vertex_count();
        let next_tetra_id = space.inner.read().next_tetra_id;
        let (tetra, positions) = make_tetra(0, Point3::new(20.0, 20.0, 20.0));
        assert_eq!(
            space.add_tetrahedron(&tetra, &positions).unwrap_err(),
            "vertex ID space exhausted"
        );
        assert_eq!(space.vertex_count(), port_count);
        assert_eq!(space.tetra_count(), 0);
        assert_eq!(space.inner.read().next_tetra_id, next_tetra_id);
        assert_vertex_grid_consistent(&space);
    }

    #[test]
    fn exhausted_vertex_id_does_not_remove_tetra_during_relocation() {
        let space = Space::new();
        let (tetra, positions) = make_tetra(0, Point3::new(20.0, 20.0, 20.0));
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        let before = space.get_tetrahedron(id).unwrap();
        let vertex_count = space.vertex_count();
        let structure_version = space.structure_version();
        space.inner.write().next_vertex_id = u64::MAX;

        assert_eq!(
            space
                .relocate_tetrahedron(id, Point3::new(30.0, 30.0, 30.0))
                .unwrap_err(),
            "vertex ID space exhausted"
        );
        assert_eq!(
            space.get_tetrahedron(id).unwrap().vertex_ids,
            before.vertex_ids
        );
        assert_eq!(space.get_tetrahedron(id).unwrap().core, before.core);
        assert_eq!(space.vertex_count(), vertex_count);
        assert_eq!(space.structure_version(), structure_version);
        assert_vertex_grid_consistent(&space);
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
    fn removing_port_anchor_preserves_port_and_reanchors_remaining_tetra() {
        let space = Space::new();
        let port_count = space.cylinder_port_count();
        let (port_vid, port_pos) = space.cylinder_ports()[0];
        let core = center_with_vertex_at(port_pos);
        let (tetra, positions) = make_tetra(0, core);
        let anchor = space.add_tetrahedron(&tetra, &positions).unwrap();
        let sibling = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert!(space.assign_specific_port(port_vid, anchor));

        space.remove_tetrahedron(anchor).unwrap();
        assert_eq!(space.tetras_connected_to_port(port_vid), vec![sibling]);
        assert_eq!(
            space
                .inner
                .read()
                .cylinder
                .find_port_for_tetra(sibling)
                .map(|p| p.id),
            Some(port_vid)
        );
        assert_vertex_grid_consistent(&space);

        space.remove_tetrahedron(sibling).unwrap();
        assert_eq!(space.vertex_count(), port_count);
        assert!(space.inner.read().vertices.contains_key(&port_vid));
        assert_vertex_grid_consistent(&space);

        let replacement = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert!(
            space
                .get_tetrahedron(replacement)
                .unwrap()
                .vertex_ids
                .contains(&port_vid),
            "the Cylinder Port must still be available for geometric merging"
        );
    }

    #[test]
    fn relocating_port_anchor_reconciles_old_and_new_port_occupancy() {
        let space = Space::new();
        let ports = space.cylinder_ports();
        let (old_port, old_position) = ports[0];
        let (new_port, new_position) = ports[1];
        let old_core = center_with_vertex_at(old_position);
        let (tetra, positions) = make_tetra(0, old_core);
        let anchor = space.add_tetrahedron(&tetra, &positions).unwrap();
        let sibling = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert!(space.assign_specific_port(old_port, anchor));

        space
            .relocate_tetrahedron(anchor, center_with_vertex_at(new_position))
            .unwrap();
        let inner = space.inner.read();
        assert_eq!(
            inner.cylinder.find_port_for_tetra(sibling).map(|p| p.id),
            Some(old_port)
        );
        assert_eq!(
            inner.cylinder.find_port_for_tetra(anchor).map(|p| p.id),
            Some(new_port)
        );
        assert!(inner.vertices.contains_key(&old_port));
        assert!(inner.vertices.contains_key(&new_port));
        drop(inner);
        assert_vertex_grid_consistent(&space);

        space
            .relocate_tetrahedron(anchor, Point3::new(20.0, 20.0, 20.0))
            .unwrap();
        assert!(space
            .inner
            .read()
            .cylinder
            .find_port_for_tetra(anchor)
            .is_none());
        assert!(space.inner.read().vertices.contains_key(&new_port));
        assert_vertex_grid_consistent(&space);
    }

    #[test]
    fn invalid_relocation_keeps_existing_topology_and_port_occupancy() {
        let space = Space::new();
        let (port_vid, port_pos) = space.cylinder_ports()[0];
        let (tetra, positions) = make_tetra(0, center_with_vertex_at(port_pos));
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert!(space.assign_specific_port(port_vid, id));
        let before = space.get_tetrahedron(id).unwrap();
        let version = space.structure_version();
        let vertex_count = space.vertex_count();

        assert!(space
            .relocate_tetrahedron(id, Point3::new(f64::NAN, 0.0, 0.0))
            .is_err());
        let after = space.get_tetrahedron(id).unwrap();
        assert_eq!(after.core, before.core);
        assert_eq!(after.vertex_ids, before.vertex_ids);
        assert_eq!(space.structure_version(), version);
        assert_eq!(space.vertex_count(), vertex_count);
        assert_eq!(space.tetras_connected_to_port(port_vid), vec![id]);
        assert_eq!(
            space
                .inner
                .read()
                .cylinder
                .find_port_for_tetra(id)
                .map(|p| p.id),
            Some(port_vid)
        );
        assert_vertex_grid_consistent(&space);
    }

    #[test]
    fn relocating_reseeded_cluster_keeps_its_logical_port() {
        let space = Space::new();
        let (tetra, positions) = make_tetra(0, Point3::new(20.0, 20.0, 0.5));
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert_eq!(space.reseed_ports(), 1);
        let port_vid = space
            .inner
            .read()
            .cylinder
            .find_port_for_tetra(id)
            .expect("isolated cluster was reseeded")
            .id;
        assert!(!space
            .get_tetrahedron(id)
            .unwrap()
            .vertex_ids
            .contains(&port_vid));

        space
            .relocate_tetrahedron(id, Point3::new(21.0, 20.0, 0.5))
            .unwrap();
        assert_eq!(
            space
                .inner
                .read()
                .cylinder
                .find_port_for_tetra(id)
                .map(|p| p.id),
            Some(port_vid)
        );
        assert_eq!(space.port_vertex_of_tetra(id), Some(port_vid));
        assert_eq!(space.tetras_connected_to_port(port_vid), vec![id]);
        assert_vertex_grid_consistent(&space);
    }

    #[test]
    fn reseed_assigns_stable_logical_edges_in_the_actual_zones() {
        let space = Space::new();
        let cases = [
            (
                Point3::new(20.0, 20.0, 1.0),
                super::super::cylinder::CylinderLayer::Instinct,
            ),
            (
                Point3::new(30.0, 20.0, 1.0),
                super::super::cylinder::CylinderLayer::Instinct,
            ),
            (
                Point3::new(40.0, 20.0, 3.0),
                super::super::cylinder::CylinderLayer::Relation,
            ),
        ];
        let ids: Vec<_> = cases
            .iter()
            .map(|(center, _)| {
                let (tetra, positions) = make_tetra(0, *center);
                space.add_tetrahedron(&tetra, &positions).unwrap()
            })
            .collect();
        let (identity_tetra, identity_positions) = make_tetra(0, Point3::new(50.0, 20.0, 11.0));
        let identity_id = space
            .add_tetrahedron(&identity_tetra, &identity_positions)
            .unwrap();

        assert_eq!(space.reseed_ports(), cases.len());
        assert_eq!(space.port_vertex_of_tetra(identity_id), None);
        let mut ports = Vec::new();
        for (&id, (_, expected_layer)) in ids.iter().zip(cases.iter()) {
            let inner = space.inner.read();
            let port = inner.cylinder.find_port_for_tetra(id).unwrap();
            assert_eq!(port.layer, *expected_layer);
            let port_vid = port.id;
            drop(inner);
            assert!(!space
                .get_tetrahedron(id)
                .unwrap()
                .vertex_ids
                .contains(&port_vid));
            assert_eq!(space.tetras_connected_to_port(port_vid), vec![id]);
            ports.push(port_vid);
        }
        assert!(ports[0] < ports[1]);
        let structure_version = space.structure_version();
        assert_eq!(space.reseed_ports(), 0);
        assert_eq!(space.structure_version(), structure_version);
        for (&id, &port_vid) in ids.iter().zip(ports.iter()) {
            assert_eq!(space.tetras_connected_to_port(port_vid), vec![id]);
        }
    }

    #[test]
    fn reseed_claims_an_existing_geometric_port_before_creating_a_virtual_link() {
        let space = Space::new();
        let (port_vid, port_pos) = space.cylinder_ports()[0];
        let (tetra, positions) = make_tetra(0, center_with_vertex_at(port_pos));
        let id = space.add_tetrahedron(&tetra, &positions).unwrap();
        assert!(space
            .get_tetrahedron(id)
            .unwrap()
            .vertex_ids
            .contains(&port_vid));
        assert_eq!(space.reseed_ports(), 1);
        assert_eq!(space.tetras_connected_to_port(port_vid), vec![id]);
        assert_eq!(space.port_stats().0, 1);
        assert_eq!(space.reseed_ports(), 0);
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
