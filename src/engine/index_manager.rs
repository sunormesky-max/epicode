use std::collections::{HashMap, HashSet};
use parking_lot::{Mutex, RwLock};

use crate::domain::space::Space;
use crate::domain::tetra::TetraId;
use crate::domain::vertex::Point3;

use super::hnsw::HnswIndex;
use super::vector::EMBEDDING_DIM;

pub struct IndexManager {
    pub label_index: Mutex<HashMap<String, Vec<TetraId>>>,
    pub content_hash_index: Mutex<HashMap<u64, TetraId>>,
    pub dirty_set: Mutex<HashSet<TetraId>>,
    pub placement_cache: Mutex<HashMap<Vec<String>, Point3>>,
    pub placement_cache_order: Mutex<std::collections::VecDeque<Vec<String>>>,
}

impl IndexManager {
    pub fn new(
        label_idx: HashMap<String, Vec<TetraId>>,
        chash_idx: HashMap<u64, TetraId>,
    ) -> Self {
        Self {
            label_index: Mutex::new(label_idx),
            content_hash_index: Mutex::new(chash_idx),
            dirty_set: Mutex::new(HashSet::new()),
            placement_cache: Mutex::new(HashMap::new()),
            placement_cache_order: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub fn mark_dirty(&self, id: TetraId) {
        self.dirty_set.lock().insert(id);
    }

    pub fn drain_dirty(&self) -> Vec<TetraId> {
        self.dirty_set.lock().drain().collect()
    }

    pub fn invalidate_placement_cache(&self) {
        self.placement_cache.lock().clear();
        self.placement_cache_order.lock().clear();
    }

    pub fn update_label_index(&self, id: TetraId, old_labels: &[String], new_labels: &[String]) {
        let mut idx = self.label_index.lock();
        for label in old_labels {
            if let Some(list) = idx.get_mut(label) {
                list.retain(|&x| x != id);
                if list.is_empty() {
                    idx.remove(label);
                }
            }
        }
        for label in new_labels {
            idx.entry(label.clone()).or_default().push(id);
        }
    }

    pub fn remove_from_label_index(&self, id: TetraId, labels: &[String]) {
        let mut idx = self.label_index.lock();
        for label in labels {
            if let Some(list) = idx.get_mut(label) {
                list.retain(|&x| x != id);
                if list.is_empty() {
                    idx.remove(label);
                }
            }
        }
    }

    pub fn remove_from_content_hash(&self, id: TetraId) {
        let mut idx = self.content_hash_index.lock();
        idx.retain(|_, &mut v| v != id);
    }

    pub fn remove_dirty(&self, id: TetraId) {
        self.dirty_set.lock().remove(&id);
    }

    pub fn insert_content_hash(&self, hash: u64, id: TetraId) {
        self.content_hash_index.lock().entry(hash).or_insert(id);
    }

    pub fn check_content_hash(&self, hash: u64) -> Option<TetraId> {
        self.content_hash_index.lock().get(&hash).copied()
    }

    pub fn insert_labels(&self, id: TetraId, labels: &[String]) {
        let mut idx = self.label_index.lock();
        for label in labels {
            idx.entry(label.clone()).or_default().push(id);
        }
    }

    pub fn cache_placement(&self, labels: &[String], pos: Point3) {
        let mut cache = self.placement_cache.lock();
        let mut order = self.placement_cache_order.lock();
        let mut sorted = labels.to_vec();
        sorted.sort();
        cache.insert(sorted.clone(), pos);
        order.push_back(sorted);
        while cache.len() > 500 {
            if let Some(old) = order.pop_front() {
                cache.remove(&old);
            } else { break; }
        }
    }

    pub fn get_cached_placement(&self, labels: &[String]) -> Option<Point3> {
        let cache = self.placement_cache.lock();
        let mut sorted = labels.to_vec();
        sorted.sort();
        cache.get(&sorted).copied()
    }

    pub fn rebuild_hnsw(&self, hnsw: &RwLock<HnswIndex>, space: &Space) {
        let tetras = space.all_tetrahedrons();
        // M1修复:锁外构建完整新索引,再一次性 swap 进去。
        // 之前在 write lock 内逐个 insert,N=1200 时持锁数百毫秒到秒级,期间所有 search_knn 全阻塞。
        let mut new_index = HnswIndex::new(EMBEDDING_DIM, 16, 100); // P1-7: ef_construction 200→100;
        for t in &tetras {
            if !t.data.embedding.is_empty() && t.data.embedding.len() == EMBEDDING_DIM {
                new_index.insert(t.id, t.data.embedding.clone());
            }
        }
        // 锁持有时间从"重建全程"降到"一次指针交换"
        *hnsw.write() = new_index;
    }
}
