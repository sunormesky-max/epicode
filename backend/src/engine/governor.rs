use crate::domain::space::Space;
use crate::domain::tetra::MemoryPayload;
use crate::engine::knowledge::KnowledgeGraph;

pub struct GovernorResult {
    pub recurrent_ids: Vec<u64>,
    pub should_consolidate: bool,
    pub should_archive: bool,
    pub should_merge: bool,
    pub contradictions: Vec<(u64, u64, f64)>,
    pub decay_applied: usize,
}

pub struct MergeCandidate {
    pub keep_id: u64,
    pub remove_id: u64,
    pub merged_labels: Vec<String>,
    pub merged_content: String,
}

pub struct LifecycleGovernor;

impl LifecycleGovernor {
    const RECURRENCE_THRESHOLD: u32 = 3;
    const ARCHIVE_AGE_DAYS: i64 = 30;
    const IMPORTANCE_DECAY_RATE: f64 = 0.98;
    const MIN_IMPORTANCE: f64 = 0.3;

    pub fn evaluate(space: &Space, knowledge: &KnowledgeGraph) -> GovernorResult {
        let tetras = space.all_tetrahedrons();
        let now = chrono::Utc::now().timestamp();
        let day_secs: i64 = 86400;

        let mut recurrent_ids = Vec::new();
        let mut should_archive = false;
        let mut has_duplicates = false;
        let mut decay_applied = 0;

        for tetra in &tetras {
            if tetra.data.enforced {
                continue;
            }

            // Recurrence tracking
            if tetra.data.access_count >= Self::RECURRENCE_THRESHOLD {
                recurrent_ids.push(tetra.id);
            }

            // Archive evaluation — importance-aware
            let age_days = (now - tetra.data.timestamp) / day_secs;
            let effective_importance = Self::effective_importance(&tetra.data, age_days);
            if age_days > Self::ARCHIVE_AGE_DAYS
                && effective_importance < Self::MIN_IMPORTANCE
                && tetra.data.content.len() < 30
            {
                should_archive = true;
            }

            // Short junk detection
            if tetra.data.content.len() < 10
                && effective_importance < 0.2
                && tetra.data.access_count == 0
                && age_days > 7
            {
                should_archive = true;
            }
        }

        // Importance decay — apply to old, unaccessed memories
        for tetra in &tetras {
            if tetra.data.enforced {
                continue;
            }
            let age_days = (now - tetra.data.timestamp) / day_secs;
            if age_days > 14 && tetra.data.access_count == 0 {
                if let Some(updated) = Self::apply_decay(space, tetra.id, &tetra.data, age_days) {
                    let _ = space.update_payload(tetra.id, updated);
                    decay_applied += 1;
                }
            }
        }

        // Duplicate detection — content hash based for top N
        if tetras.len() > 20 {
            let check_limit = tetras.len().min(100);
            for i in 0..check_limit {
                for j in (i + 1)..check_limit {
                    let ci = &tetras[i].data.content;
                    let cj = &tetras[j].data.content;
                    if ci.len() > 20 && cj.len() > 20 {
                        if ci == cj || Self::fuzzy_content_match(ci, cj) {
                            has_duplicates = true;
                            break;
                        }
                    }
                }
                if has_duplicates {
                    break;
                }
            }
        }

        // Contradiction detection
        let mut contradictions = Vec::new();
        let negation_indicators = [
            "不是",
            "不能",
            "错误",
            "修正",
            "已修正",
            "fix",
            "fixed",
            "wrong",
            "incorrect",
            "不再",
            "改为",
            "instead of",
            "替代",
            "deprecated",
            "废弃",
            "移除",
            "removed",
        ];

        for i in 0..tetras.len().min(80) {
            for j in (i + 1)..tetras.len().min(80) {
                let ci = &tetras[i].data;
                let cj = &tetras[j].data;

                if ci.content.len() < 30 || cj.content.len() < 30 {
                    continue;
                }
                if ci.enforced || cj.enforced {
                    continue;
                }

                let topic_overlap = Self::content_overlap(&ci.content, &cj.content);
                if topic_overlap < 0.08 {
                    continue;
                }

                let i_has_negation = negation_indicators
                    .iter()
                    .any(|w| ci.content.to_lowercase().contains(w));
                let j_has_negation = negation_indicators
                    .iter()
                    .any(|w| cj.content.to_lowercase().contains(w));

                if (i_has_negation || j_has_negation) && !(i_has_negation && j_has_negation) {
                    contradictions.push((tetras[i].id, tetras[j].id, topic_overlap));
                }
            }
        }

        if !contradictions.is_empty() {
            tracing::info!(
                "[Governor] found {} contradiction pairs",
                contradictions.len()
            );
            for &(a, b, sim) in &contradictions {
                let _ = knowledge.add_relation(
                    a,
                    b,
                    crate::engine::knowledge::RelationType::Contradicts,
                    sim,
                );
            }
        }

        let should_consolidate = !recurrent_ids.is_empty();

        tracing::info!(
            "[Governor] recurrent={}/{} should_consolidate={} should_archive={} should_merge={} decayed={}",
            recurrent_ids.len(), tetras.len(), should_consolidate, should_archive, has_duplicates, decay_applied
        );

        GovernorResult {
            recurrent_ids,
            should_consolidate,
            should_archive,
            should_merge: has_duplicates,
            contradictions,
            decay_applied,
        }
    }

    fn effective_importance(payload: &MemoryPayload, age_days: i64) -> f64 {
        let base = payload.importance;
        let access_bonus = (payload.access_count as f64).ln().max(0.0) * 0.3;
        let recency_factor = if age_days > 0 {
            Self::IMPORTANCE_DECAY_RATE.powi(age_days as i32)
        } else {
            1.0
        };
        (base + access_bonus) * recency_factor
    }

    fn apply_decay(
        _space: &Space,
        id: u64,
        data: &MemoryPayload,
        age_days: i64,
    ) -> Option<MemoryPayload> {
        let decayed = Self::effective_importance(data, age_days);
        if (data.importance - decayed).abs() > 0.05 {
            let mut updated = data.clone();
            updated.importance = decayed.max(0.1);
            tracing::info!(
                "[Governor] decayed #{}: importance {:.2} -> {:.2} (age={}d)",
                id,
                data.importance,
                updated.importance,
                age_days
            );
            Some(updated)
        } else {
            None
        }
    }

    fn fuzzy_content_match(a: &str, b: &str) -> bool {
        if a.len().abs_diff(b.len()) > a.len() / 3 {
            return false;
        }
        let overlap = Self::content_overlap(a, b);
        overlap > 0.85
    }

    fn content_overlap(a: &str, b: &str) -> f64 {
        let lower_a = a.to_lowercase();
        let lower_b = b.to_lowercase();
        let set_a: std::collections::HashSet<&str> = lower_a
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        let set_b: std::collections::HashSet<&str> = lower_b
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 2)
            .collect();
        if set_a.is_empty() || set_b.is_empty() {
            return 0.0;
        }
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        if union == 0 {
            return 0.0;
        }
        intersection as f64 / union as f64
    }

    pub fn find_merge_candidates(space: &Space) -> Vec<MergeCandidate> {
        let tetras = space.all_tetrahedrons();
        let mut candidates = Vec::new();
        let mut merged_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for i in 0..tetras.len() {
            if merged_ids.contains(&tetras[i].id) {
                continue;
            }
            for j in (i + 1)..tetras.len() {
                if merged_ids.contains(&tetras[j].id) {
                    continue;
                }

                let ci = &tetras[i].data.content;
                let cj = &tetras[j].data.content;
                if ci == cj || Self::fuzzy_content_match(ci, cj) {
                    let (keep, remove) = if tetras[i].data.timestamp >= tetras[j].data.timestamp {
                        (&tetras[i], &tetras[j])
                    } else {
                        (&tetras[j], &tetras[i])
                    };

                    let mut merged_labels = keep.data.labels.clone();
                    for label in &remove.data.labels {
                        if !merged_labels.contains(label) {
                            merged_labels.push(label.clone());
                        }
                    }

                    candidates.push(MergeCandidate {
                        keep_id: keep.id,
                        remove_id: remove.id,
                        merged_labels,
                        merged_content: keep.data.content.clone(),
                    });

                    merged_ids.insert(remove.id);
                    break;
                }
            }
        }

        tracing::info!("[Governor] found {} merge candidates", candidates.len());
        candidates
    }

    pub fn execute_merges(space: &Space, candidates: &[MergeCandidate]) -> usize {
        let mut merged_count = 0;
        for candidate in candidates {
            if let Some(tetra) = space.get_tetrahedron(candidate.keep_id) {
                let mut data = tetra.data.clone();
                data.labels = candidate.merged_labels.clone();
                let _ = space.update_payload(candidate.keep_id, data);
            }
            let _ = space.remove_tetrahedron(candidate.remove_id);
            tracing::info!(
                "[Governor] merged #{} into #{}",
                candidate.remove_id,
                candidate.keep_id
            );
            merged_count += 1;
        }
        merged_count
    }

    pub fn reset_access_counts(space: &Space, ids: &[u64]) {
        for &id in ids {
            if let Some(tetra) = space.get_tetrahedron(id) {
                let mut data = tetra.data.clone();
                let old = data.access_count;
                data.access_count = 0;
                let _ = space.update_payload(id, data);
                tracing::info!("[Governor] reset access_count for #{}: {} -> 0", id, old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tetra::{MemoryPayload, Tetrahedron};
    use crate::domain::vertex::Point3;

    fn make_tetra(space: &Space, content: &str, importance: f64, access_count: u32) {
        let core = Point3::new(0.0, 0.0, 0.0);
        let pos = Tetrahedron::compute_vertices(core);
        let data = MemoryPayload {
            content: content.to_string(),
            content_hash: 0,
            labels: vec![],
            timestamp: chrono::Utc::now().timestamp() - 86400 * 31,
            aliases: vec![],
            embedding: vec![],
            importance,
            enforced: false,
            rationale: None,
            access_count,
            memory_type: None,
        };
        let t = Tetrahedron {
            id: 0,
            vertex_ids: [0; 4],
            core,
            data,
            mass: 1.0,
        };
        let _ = space.add_tetrahedron(&t, &pos);
    }

    #[test]
    fn recurrent_detection() {
        let space = Space::new();
        make_tetra(&space, "important decision", 2.0, 5);
        make_tetra(&space, "minor note", 0.5, 0);

        let kg = KnowledgeGraph::new();
        let result = LifecycleGovernor::evaluate(&space, &kg);
        assert!(result.should_consolidate);
        assert_eq!(result.recurrent_ids.len(), 1);
    }

    #[test]
    fn no_consolidate_when_no_recurrence() {
        let space = Space::new();
        make_tetra(&space, "note 1", 1.0, 0);
        make_tetra(&space, "note 2", 1.0, 1);

        let kg = KnowledgeGraph::new();
        let result = LifecycleGovernor::evaluate(&space, &kg);
        assert!(!result.should_consolidate);
    }

    #[test]
    fn fuzzy_duplicate_detection() {
        assert!(LifecycleGovernor::fuzzy_content_match(
            "Deploy epicode using atomic replace: cp, stop, mv, start",
            "Deploy epicode using atomic replace: cp, stop, mv, start"
        ));
        assert!(!LifecycleGovernor::fuzzy_content_match(
            "Fix React crash on page load",
            "Deploy new nginx configuration"
        ));
    }

    #[test]
    fn effective_importance_decay() {
        let payload = MemoryPayload {
            content: "test".to_string(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 2.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
        };
        let young = LifecycleGovernor::effective_importance(&payload, 1);
        let old = LifecycleGovernor::effective_importance(&payload, 100);
        assert!(young > old);
    }

    #[test]
    fn contradiction_detection() {
        let space = Space::new();
        let a = "Use firewalld for all port blocking and firewall rules on the server";
        let b = "Fix: removed firewalld 改为 use nft instead, firewalld causes nftables crash";
        make_tetra(&space, a, 2.0, 0);
        make_tetra(&space, b, 2.0, 0);

        let kg = KnowledgeGraph::new();
        let result = LifecycleGovernor::evaluate(&space, &kg);
        assert!(
            !result.contradictions.is_empty(),
            "should detect contradiction between firewalld vs nft"
        );
    }

    #[test]
    fn merge_candidates_detected() {
        let space = Space::new();
        let content = "Deploy using atomic replace: cp, stop, mv, start".to_string();
        make_tetra(&space, &content, 1.0, 0);
        make_tetra(&space, &content, 1.0, 0);

        let candidates = LifecycleGovernor::find_merge_candidates(&space);
        assert!(
            !candidates.is_empty(),
            "should find exact duplicate merge candidates"
        );
    }

    #[test]
    fn effective_importance_access_bonus() {
        let no_access = MemoryPayload {
            content: "test".to_string(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
        };
        let frequent = MemoryPayload {
            content: "test".to_string(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 10,
            memory_type: None,
        };
        let imp_no = LifecycleGovernor::effective_importance(&no_access, 10);
        let imp_freq = LifecycleGovernor::effective_importance(&frequent, 10);
        assert!(
            imp_freq > imp_no,
            "frequently accessed memory should have higher effective importance"
        );
    }
}
