//! S4 M5a (#398): the in-process Space4 mirror.
//!
//! Grey-dial `space4_enabled = "mirror"`: the 3D `Space` stays the system of
//! record (placement, search, KG, storage untouched — zero hot-path change),
//! while every successful create is projected into a same-process
//! [`Space4`]. Unlike the external S1 shadow (a separate binary re-reading
//! SQLite every 6h), this mirror lives next to the engine, so the upcoming
//! M6 retrieval channel can read it with zero cross-process sync.
//!
//! Lifecycle: built by replaying all tetrahedrons once (3k cells < 1s at
//! ~4400 cells/s), then incrementally fed creates/removes. Updates and
//! relocations are not mirrored incrementally — they self-heal on the next
//! restart replay; between restarts the mirror may hold stale copies of a
//! few moved cells (acceptable for mirror-mode stats and M6 ranking
//! experiments, documented deviation).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::domain::space::Space;
use crate::domain::space4::{CellPayload, PentaId, Rng, Space4, SpaceStats};
use crate::domain::tetra::{TetraId, Tetrahedron};

/// Normalization shared with storage M3 (`normalized_valid_from`).
fn normalized_valid_from(t: &Tetrahedron) -> i64 {
    if t.data.valid_from > 0 {
        t.data.valid_from
    } else {
        t.data.timestamp
    }
}

/// Project a production tetrahedron into the 4D cell payload. Layer resolves
/// through the authoritative `CylinderLayer::from_labels` at insert time
/// (inside Space4), matching the 3D placement's own routing.
fn project(t: &Tetrahedron) -> CellPayload {
    CellPayload {
        labels: t.data.labels.clone(),
        importance: t.data.importance,
        valid_from: normalized_valid_from(t),
        valid_until: t.data.valid_to,
        expired_at: t.data.expired_at,
        layer: None,
    }
}

pub struct Space4Mirror {
    space: Space4,
    rng: Mutex<Rng>,
    /// 3D tetra id → 4D cell id (mirrored creates only; replay rebuilds it).
    ids: Mutex<HashMap<TetraId, PentaId>>,
}

impl Space4Mirror {
    /// Build by replaying the whole 3D space (the same truth the S1 shadow
    /// ingests, but in-process). Deterministic given (κ, t0).
    pub fn build(space: &Space, kappa: f64, t0_secs: i64) -> Self {
        let mirror = Space4Mirror {
            space: Space4::new(kappa, t0_secs),
            rng: Mutex::new(Rng::new(0x5EED_0004)),
            ids: Mutex::new(HashMap::new()),
        };
        let tetras = space.all_tetrahedrons();
        let mut rng = mirror.rng.lock();
        for t in &tetras {
            if let Some(pid) = mirror.space.create_full(project(t), &mut rng) {
                mirror.ids.lock().insert(t.id, pid);
            }
        }
        drop(rng);
        let mirrored = mirror.ids.lock().len();
        let skipped = tetras.len() - mirrored;
        if skipped > 0 {
            tracing::warn!(
                "[S4-mirror] replay: {} of {} cells rejected by the 4D guards",
                skipped,
                tetras.len()
            );
        }
        tracing::info!(
            "[S4-mirror] replayed {} cells (κ={}, t0={})",
            mirrored,
            kappa,
            t0_secs
        );
        mirror
    }

    /// Mirror one successful 3D create. Returns the 4D cell id.
    pub fn mirror_create(&self, t: &Tetrahedron) -> Option<PentaId> {
        let mut rng = self.rng.lock();
        let pid = self.space.create_full(project(t), &mut rng)?;
        drop(rng);
        self.ids.lock().insert(t.id, pid);
        Some(pid)
    }

    /// Mirror a removal (forget/tombstone sweep). Unknown ids are ignored —
    /// replay-born mappings are not tracked cell-by-cell.
    pub fn mirror_remove(&self, tetra_id: TetraId) -> bool {
        match self.ids.lock().remove(&tetra_id) {
            Some(pid) => self.space.remove_penta(pid),
            None => false,
        }
    }

    /// Register a recall for retention stretching (M6 ranking hook).
    pub fn touch(&self, tetra_id: TetraId) {
        if let Some(&pid) = self.ids.lock().get(&tetra_id) {
            self.space.touch(pid);
        }
    }

    /// The 4D cell mirroring a 3D tetra, if any.
    pub fn penta_of(&self, tetra_id: TetraId) -> Option<PentaId> {
        self.ids.lock().get(&tetra_id).copied()
    }

    /// S4 M6: the 4D retrieval channel. Slice → era-proximal label seeds →
    /// Space-PPR over the vertex-sharing graph → 0.6·seed + 0.4·ppr fusion.
    /// Constants aligned with the KG graph mode (0.15 / 15 iter / 800
    /// frontier). Touches returned cells (recall feeds retention stretch).
    /// Returns (tetra_id, fused_score) — callers resolve payloads from the
    /// 3D space (the system of record).
    pub fn search_labels(
        &self,
        q_labels: &[String],
        as_of_secs: i64,
        k: usize,
    ) -> Vec<(TetraId, f64)> {
        const PPR_SEED: f64 = 0.15;
        const PPR_ITER: usize = 15;
        const PPR_FRONTIER: usize = 800;
        const FUSION_SEED: f64 = 0.6;
        const ERA_BETA: f64 = 2.0;
        const STALE_SEED_FACTOR: f64 = 0.5;

        let w_q = crate::domain::geo4::w_of_unix_secs(as_of_secs, self.space.t0_secs);

        // 1) one-lock snapshot slice → seeds (label overlap × era proximity)
        let snap = self.space.snapshot_valid(q_labels, as_of_secs);
        let mut seeds: HashMap<PentaId, f64> = HashMap::new();
        for s in &snap {
            if s.label_overlap == 0 || q_labels.is_empty() {
                continue;
            }
            let overlap = s.label_overlap as f64 / q_labels.len().max(s.labels_len) as f64;
            let era = (-(s.core_w - w_q).abs() * ERA_BETA).exp();
            let mut score = overlap * era;
            if s.validity == crate::domain::space4::Validity::BelievedStale {
                score *= STALE_SEED_FACTOR;
            }
            if score > 0.0 {
                seeds.insert(s.id, score);
            }
        }
        if seeds.is_empty() {
            return Vec::new();
        }

        // 2) bounded BFS over the sharing graph from the seeds
        let adj_all = self.space.sharing_adjacency_cached();
        let mut seen_set: HashMap<PentaId, bool> = seeds.keys().map(|&s| (s, true)).collect();
        let mut seen: Vec<PentaId> = seeds.keys().copied().collect();
        seen.sort_unstable();
        let mut queue: Vec<PentaId> = seen.clone();
        while let Some(cur) = queue.pop() {
            if seen.len() >= PPR_FRONTIER {
                break;
            }
            if let Some(nbrs) = adj_all.get(&cur) {
                for (n, _) in nbrs {
                    if !seen_set.get(n).copied().unwrap_or(false) {
                        seen_set.insert(*n, true);
                        seen.push(*n);
                        queue.push(*n);
                    }
                }
            }
        }

        // 3) power iteration: rank = 0.15·seed + 0.85·Mᵀ·rank
        let mut rank: HashMap<PentaId, f64> = HashMap::new();
        for &id in &seen {
            rank.insert(id, *seeds.get(&id).unwrap_or(&0.0));
        }
        for _ in 0..PPR_ITER {
            let mut next: HashMap<PentaId, f64> = HashMap::new();
            for (&id, &s) in &seeds {
                *next.entry(id).or_insert(0.0) += PPR_SEED * s;
            }
            for &src in &seen {
                let Some(nbrs) = adj_all.get(&src) else {
                    continue;
                };
                let filt: Vec<&(PentaId, f64)> = nbrs
                    .iter()
                    .filter(|(b, _)| seen_set.get(b).copied().unwrap_or(false))
                    .collect();
                if filt.is_empty() {
                    continue;
                }
                let r = *rank.get(&src).unwrap_or(&0.0);
                if r <= 0.0 {
                    continue;
                }
                let wsum: f64 = filt.iter().map(|(_, w)| *w).sum();
                for (dst, w) in filt {
                    *next.entry(*dst).or_insert(0.0) += (1.0 - PPR_SEED) * r * (w / wsum);
                }
            }
            rank = next;
        }

        // 4) fusion + top-k, mapped back to 3D tetra ids
        let ppr_max = rank.values().cloned().fold(0.0_f64, f64::max).max(1e-12);
        let seed_max = seeds.values().cloned().fold(0.0_f64, f64::max).max(1e-12);
        let reverse: HashMap<PentaId, TetraId> =
            self.ids.lock().iter().map(|(&t, &p)| (p, t)).collect();
        let mut hits: Vec<(TetraId, f64)> = rank
            .into_iter()
            .filter_map(|(pid, ppr)| {
                let tetra = reverse.get(&pid).copied()?;
                let s = *seeds.get(&pid).unwrap_or(&0.0);
                let fused = FUSION_SEED * (s / seed_max) + (1.0 - FUSION_SEED) * (ppr / ppr_max);
                Some((tetra, fused))
            })
            .collect();
        hits.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        hits.truncate(k);
        // Recall registration feeds retention stretch (lifecycle hook).
        for (tetra, _) in &hits {
            self.touch(*tetra);
        }
        hits
    }

    /// Structural stats (T1′ census, era purity — the S2 readout basis).
    pub fn stats(&self) -> SpaceStats {
        self.space.stats()
    }

    /// Direct handle for the M6 retrieval channel.
    pub fn space(&self) -> &Space4 {
        &self.space
    }

    /// Build from the storage meta dials installed by S4 M3:
    /// `space4_enabled == "mirror"` → Some (replayed), anything else → None.
    pub fn from_storage_dials(
        storage: &crate::engine::storage::StorageManager,
        space: &Space,
    ) -> Option<Arc<Self>> {
        let enabled = storage.get_meta("space4_enabled").unwrap_or_default();
        if enabled != "mirror" {
            return None;
        }
        let kappa: f64 = storage
            .get_meta("space4_kappa")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let t0: i64 = storage
            .get_meta("space_t0_secs")
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                // No anchor yet (empty space): defer to the first write's
                // lazy anchoring — t0 = now keeps w ≈ 0 until then.
                chrono::Utc::now().timestamp()
            });
        Some(Arc::new(Self::build(space, kappa, t0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::MemoryPayload;
    use crate::domain::vertex::Point3;

    const T0: i64 = 1_767_225_600;

    fn tetra(id: u64, labels: &[&str], day: i64) -> Tetrahedron {
        Tetrahedron {
            id,
            vertex_ids: [0; 4],
            core: Point3::new(id as f64, 0.0, 0.0),
            data: MemoryPayload {
                content: format!("mem-{id}"),
                content_hash: id * 97,
                labels: labels.iter().map(|s| s.to_string()).collect(),
                timestamp: T0 + day * 86_400,
                aliases: vec![],
                embedding: vec![],
                importance: 1.0,
                enforced: false,
                rationale: None,
                access_count: 0,
                memory_type: None,
                identity_stamp: None,
                source_agent: None,
                valid_from: 0,
                valid_to: None,
                last_reviewed_ts: None,
                expired_at: None,
                invalidated_at: None,
                memory_class: None,
            },
            mass: 1.0,
        }
    }

    fn seeded_space() -> Space {
        let space = Space::new();
        for i in 0..6u64 {
            let t = tetra(i, &["kg", &format!("topic-{i}")], 10);
            let pos = Tetrahedron::compute_vertices(t.core);
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        space
    }

    #[test]
    fn replay_then_mirror_create_and_remove() {
        let space = seeded_space();
        let mirror = Space4Mirror::build(&space, 1.0, T0);
        let st = mirror.stats();
        assert_eq!(st.pentas, 6, "all replayed cells land");
        // Same-topic same-era cells dock into clusters; the 7% orphan
        // diversity seed may split the chain — structural invariants only.
        assert!(st.clusters >= 1 && st.clusters <= 6);
        assert!(
            st.largest_cluster >= 2,
            "anchored placement docks same-topic cells: {:?}",
            st
        );
        // T1′ support holds on replayed production shapes.
        for k in st.pair_sharing_hist.keys() {
            assert!(matches!(k, 0 | 1 | 5), "T1′ violated: share {k}");
        }

        // Incremental create mirrors and maps.
        let t7 = tetra(7, &["kg"], 10);
        let pid7 = mirror.mirror_create(&t7).expect("mirror create lands");
        assert_eq!(mirror.penta_of(7), Some(pid7));
        assert_eq!(mirror.stats().pentas, 7);

        // Touch + remove round-trip.
        mirror.touch(7);
        assert!(mirror.mirror_remove(7));
        assert_eq!(mirror.penta_of(7), None);
        assert_eq!(mirror.stats().pentas, 6);
        assert!(!mirror.mirror_remove(7), "second remove is a no-op");
    }

    #[test]
    fn kappa_zero_mirror_is_degenerate_safe() {
        // κ = 0 collapses the metric onto xyz — the mirror must still build
        // and never panic (guards all operate in the degenerate metric).
        let space = seeded_space();
        let mirror = Space4Mirror::build(&space, 0.0, T0);
        assert_eq!(mirror.stats().pentas, 6);
    }

    #[test]
    fn handle_exposes_engine_for_m6() {
        let mirror = Space4Mirror::build(&seeded_space(), 1.0, T0);
        let snap = mirror
            .space()
            .snapshot_valid(&["kg".to_string()], T0 + 11 * 86_400);
        assert_eq!(snap.len(), 6, "all cells valid at day 11 (no expiry set)");
    }

    #[test]
    fn m6_search_labels_ranks_topic_and_maps_back() {
        // A space with two topics in two eras; query the old era of "kg".
        let space = Space::new();
        for i in 0..3u64 {
            let t = tetra(i, &["kg", &format!("k-{i}")], 10);
            let pos = Tetrahedron::compute_vertices(t.core);
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        for i in 10..12u64 {
            let t = tetra(i, &["ai", &format!("a-{i}")], 200);
            let pos = Tetrahedron::compute_vertices(t.core);
            space.add_tetrahedron(&t, &pos).unwrap();
        }
        let mirror = Space4Mirror::build(&space, 1.0, T0);

        // as_of near day 10: kg-topic cells must outrank the far-era ai ones.
        let hits = mirror.search_labels(&["kg".to_string()], T0 + 12 * 86_400, 5);
        assert!(!hits.is_empty(), "bridged labels must retrieve");
        assert!(
            hits.iter().all(|(id, _)| *id < 3),
            "era proximity must rank the kg era on top: {hits:?}"
        );
        // Recall registration fires (access counts grow).
        assert!(
            mirror
                .space()
                .get(mirror.penta_of(hits[0].0).unwrap())
                .map(|p| p.access_count >= 1)
                .unwrap_or(false),
            "search touches its hits"
        );
    }

    #[test]
    fn m6_bridge_extracts_labels_from_free_text() {
        use std::collections::HashMap;
        let mut idx: HashMap<String, Vec<TetraId>> = HashMap::new();
        for lab in ["kg", "ai", "deploy", "identity-boundary", "数据库"] {
            idx.insert(lab.to_string(), vec![1]);
        }
        let q1 = crate::engine::search_engine::bridge_query_labels(
            "how did the kg deploy go",
            &Mutex::new(idx.clone()),
        );
        assert!(q1.contains(&"kg".to_string()) && q1.contains(&"deploy".to_string()));
        let q2 =
            crate::engine::search_engine::bridge_query_labels("数据库迁移方案", &Mutex::new(idx));
        assert!(
            q2.contains(&"数据库".to_string()),
            "CJK substring match works"
        );
        let q3 = crate::engine::search_engine::bridge_query_labels(
            "nothing matches here",
            &Mutex::new(HashMap::new()),
        );
        assert!(q3.is_empty(), "empty vocabulary bridges to nothing");
    }
}
