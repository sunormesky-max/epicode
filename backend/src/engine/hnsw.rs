use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// HNSW (Hierarchical Navigable Small World) index for fast approximate
/// nearest neighbor search in embedding space.
///
/// Based on the HNSW paper (Malkov & Yashunin, 2016).
/// Parameters:
/// - M: max connections per node per layer (default 16)
/// - ef_construction: search width during insertion (default 200)
/// - ef_search: search width during query (default 50)
pub struct HnswIndex {
    nodes: HashMap<u64, HnswNode>,
    entry_point: Option<u64>,
    max_level: usize,
    _m: usize,
    m_max: usize,
    m_max0: usize,
    ef_construction: usize,
    ml: f64,
    _dims: usize,
}

#[derive(Debug, Clone)]
struct HnswNode {
    _id: u64,
    embedding: Vec<f64>,
    connections: Vec<HashSet<u64>>,
}

impl HnswIndex {
    pub fn new(dims: usize, m: usize, ef_construction: usize) -> Self {
        let m_max = m;
        let m_max0 = m * 2;
        Self {
            nodes: HashMap::new(),
            entry_point: None,
            max_level: 0,
            _m: m,
            m_max,
            m_max0,
            ef_construction,
            ml: 1.0 / (m as f64).ln(),
            _dims: dims,
        }
    }

    pub fn insert(&mut self, id: u64, embedding: Vec<f64>) {
        if self.nodes.contains_key(&id) {
            return;
        }

        let level = self.random_level();
        let connections = vec![HashSet::new(); level + 1];

        if self.entry_point.is_none() {
            self.nodes.insert(
                id,
                HnswNode {
                    _id: id,
                    embedding,
                    connections,
                },
            );
            self.entry_point = Some(id);
            self.max_level = level;
            return;
        }

        let ep = self.entry_point.unwrap();
        let mut current = ep;
        let mut current_dist = distance(&self.nodes[&ep].embedding, &embedding);

        // Search from top level down
        for lc in (level + 1..=self.max_level).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                for &neighbor in &self.nodes[&current]
                    .connections
                    .get(lc)
                    .cloned()
                    .unwrap_or_default()
                {
                    let d = distance(&self.nodes[&neighbor].embedding, &embedding);
                    if d < current_dist {
                        current = neighbor;
                        current_dist = d;
                        changed = true;
                    }
                }
            }
        }

        // Insert node FIRST so get_mut works for wiring
        self.nodes.insert(
            id,
            HnswNode {
                _id: id,
                embedding: embedding.clone(),
                connections,
            },
        );

        // Wire connections at each level
        for lc in (0..=level.min(self.max_level)).rev() {
            let neighbors = self.search_layer(&embedding, current, self.ef_construction, lc);
            let selected = self.select_neighbors(
                &embedding,
                &neighbors,
                if lc == 0 { self.m_max0 } else { self.m_max },
                lc,
            );
            for &n in &selected {
                if let Some(node) = self.nodes.get_mut(&id) {
                    if let Some(c) = node.connections.get_mut(lc) {
                        c.insert(n);
                    }
                }
                if let Some(node) = self.nodes.get_mut(&n) {
                    if let Some(c) = node.connections.get_mut(lc) {
                        c.insert(id);
                    }
                }
            }
        }

        if level > self.max_level {
            self.max_level = level;
            self.entry_point = Some(id);
        }
    }

    pub fn search_knn(&self, query: &[f64], k: usize, ef: usize) -> Vec<(u64, f64)> {
        if self.entry_point.is_none() || k == 0 {
            return vec![];
        }

        let ep = self.entry_point.unwrap();
        let mut current = ep;
        let mut current_dist = distance(&self.nodes[&ep].embedding, query);

        for lc in (1..=self.max_level).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                if let Some(node) = self.nodes.get(&current) {
                    if let Some(conns) = node.connections.get(lc) {
                        for &neighbor in conns {
                            let d = distance(&self.nodes[&neighbor].embedding, query);
                            if d < current_dist {
                                current = neighbor;
                                current_dist = d;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        let candidates = self.search_layer(query, current, ef, 0);
        let mut sorted: Vec<(u64, f64)> = candidates
            .into_iter()
            .map(|id| (id, distance(&self.nodes[&id].embedding, query)))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        sorted.truncate(k);

        // Convert distance to similarity [0, 1]
        sorted
            .into_iter()
            .map(|(id, d)| {
                let sim = 1.0 / (1.0 + d);
                (id, sim)
            })
            .collect()
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(node) = self.nodes.remove(&id) {
            for conns in &node.connections {
                for &neighbor in conns {
                    if let Some(n) = self.nodes.get_mut(&neighbor) {
                        for level_conns in &mut n.connections {
                            level_conns.remove(&id);
                        }
                    }
                }
            }
        }
        if self.entry_point == Some(id) {
            self.entry_point = self.nodes.keys().next().copied();
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    fn random_level(&self) -> usize {
        let r: f64 = rand::random::<f64>();
        ((-r.ln() * self.ml) as usize).min(16)
    }

    fn search_layer(&self, query: &[f64], entry: u64, ef: usize, level: usize) -> Vec<u64> {
        let mut visited: HashSet<u64> = HashSet::new();
        let mut candidates = BinaryHeap::new();
        let mut results = BinaryHeap::new();

        let d = distance(&self.nodes[&entry].embedding, query);
        candidates.push(Candidate {
            id: entry,
            dist: -d,
        });
        results.push(Candidate { id: entry, dist: d });
        visited.insert(entry);

        while let Some(Candidate {
            id: current,
            dist: cand_dist,
        }) = candidates.pop()
        {
            let worst_result_dist = results.peek().map(|c| c.dist).unwrap_or(f64::MAX);
            if -cand_dist > worst_result_dist && results.len() >= ef {
                break;
            }

            if let Some(node) = self.nodes.get(&current) {
                if let Some(conns) = node.connections.get(level) {
                    for &neighbor in conns {
                        if visited.insert(neighbor) {
                            let nd = distance(&self.nodes[&neighbor].embedding, query);
                            if results.len() < ef
                                || nd < results.peek().map(|c| c.dist).unwrap_or(f64::MAX)
                            {
                                candidates.push(Candidate {
                                    id: neighbor,
                                    dist: -nd,
                                });
                                results.push(Candidate {
                                    id: neighbor,
                                    dist: nd,
                                });
                                if results.len() > ef {
                                    results.pop();
                                }
                            }
                        }
                    }
                }
            }
        }

        results.into_iter().map(|c| c.id).collect()
    }

    fn select_neighbors(
        &self,
        embedding: &[f64],
        candidates: &[u64],
        m: usize,
        _level: usize,
    ) -> Vec<u64> {
        let mut scored: Vec<(u64, f64, u64)> = candidates
            .iter()
            .map(|&id| (id, distance(&self.nodes[&id].embedding, embedding), id))
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        scored.truncate(m);
        scored.into_iter().map(|(id, _, _)| id).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: u64,
    dist: f64,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.dist.partial_cmp(&other.dist)
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

fn distance(a: &[f64], b: &[f64]) -> f64 {
    let len = a.len().min(b.len());
    let mut sum = 0.0;
    for i in 0..len {
        let d = a[i] - b[i];
        sum += d * d;
    }
    sum.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_search() {
        let mut idx = HnswIndex::new(4, 16, 200);
        idx.insert(1, vec![1.0, 0.0, 0.0, 0.0]);
        idx.insert(2, vec![1.0, 0.1, 0.0, 0.0]);
        idx.insert(3, vec![0.0, 1.0, 0.0, 0.0]);
        idx.insert(4, vec![0.0, 1.0, 0.1, 0.0]);

        let results = idx.search_knn(&[1.0, 0.0, 0.0, 0.0], 2, 50);
        assert!(!results.is_empty(), "should return at least one result");
        assert_eq!(results[0].0, 1); // closest is itself
    }

    #[test]
    fn empty_search() {
        let idx = HnswIndex::new(4, 16, 100);
        assert!(idx.search_knn(&[1.0, 0.0, 0.0, 0.0], 2, 20).is_empty());
    }

    #[test]
    fn remove_and_search() {
        let mut idx = HnswIndex::new(4, 16, 100);
        idx.insert(1, vec![1.0, 0.0, 0.0, 0.0]);
        idx.insert(2, vec![0.0, 1.0, 0.0, 0.0]);
        idx.remove(1);
        assert_eq!(idx.len(), 1);
        let r = idx.search_knn(&[1.0, 0.0, 0.0, 0.0], 1, 20);
        assert_eq!(r[0].0, 2);
    }

    #[test]
    fn similarity_increases_for_closer_points() {
        let mut idx = HnswIndex::new(4, 16, 100);
        idx.insert(1, vec![1.0, 0.0, 0.0, 0.0]);
        idx.insert(2, vec![5.0, 0.0, 0.0, 0.0]);

        let r = idx.search_knn(&[1.0, 0.0, 0.0, 0.0], 2, 20);
        assert!(r.len() >= 2, "expected at least 2 results, got {}", r.len());
        assert!(r[0].1 > r[1].1); // closer point has higher sim
    }

    #[test]
    fn many_inserts() {
        let mut idx = HnswIndex::new(8, 16, 200);
        for i in 0..100 {
            let v: Vec<f64> = (0..8).map(|j| ((i + j) as f64).sin()).collect();
            idx.insert(i as u64, v);
        }
        assert_eq!(idx.len(), 100);
        let r = idx.search_knn(&[0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5], 5, 50);
        assert_eq!(r.len(), 5);
    }
}
