use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Unique identifier for a cluster node.
pub type NodeId = uuid::Uuid;

/// Information about a cluster member.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeInfo {
    pub id: NodeId,
    pub addr: String,
    pub last_heartbeat: u64,
    pub is_local: bool,
}

/// Configuration for cluster behavior.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub listen_addr: String,
    pub seed_nodes: Vec<String>,
    pub heartbeat_interval_ms: u64,
    pub heartbeat_timeout_ms: u64,
    pub vnode_count: usize,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: "0.0.0.0:7946".to_string(),
            seed_nodes: Vec::new(),
            heartbeat_interval_ms: 1000,
            heartbeat_timeout_ms: 5000,
            vnode_count: 128,
        }
    }
}

impl ClusterConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("EPICODE_CLUSTER_ENABLED") {
            cfg.enabled = v.parse().unwrap_or(false);
        }
        if let Ok(v) = std::env::var("EPICODE_CLUSTER_LISTEN") {
            cfg.listen_addr = v;
        }
        if let Ok(v) = std::env::var("EPICODE_CLUSTER_SEEDS") {
            cfg.seed_nodes = v.split(',').map(|s| s.trim().to_string()).collect();
        }
        if let Ok(v) = std::env::var("EPICODE_CLUSTER_VNODES") {
            cfg.vnode_count = v.parse().unwrap_or(128);
        }
        cfg
    }
}

/// Consistent hash ring for user-to-node routing.
pub struct HashRing {
    vnodes: BTreeMap<u64, NodeId>,
    nodes: HashMap<NodeId, NodeInfo>,
    vnode_count: usize,
}

impl HashRing {
    pub fn new(vnode_count: usize) -> Self {
        Self {
            vnodes: BTreeMap::new(),
            nodes: HashMap::new(),
            vnode_count,
        }
    }

    pub fn add_node(&mut self, info: NodeInfo) {
        let id = info.id;
        if self.nodes.contains_key(&id) {
            return;
        }
        for i in 0..self.vnode_count {
            let key = hash_node_vnode(&id, i);
            self.vnodes.insert(key, id);
        }
        self.nodes.insert(id, info);
    }

    pub fn remove_node(&mut self, id: NodeId) {
        if !self.nodes.contains_key(&id) {
            return;
        }
        for i in 0..self.vnode_count {
            let key = hash_node_vnode(&id, i);
            self.vnodes.remove(&key);
        }
        self.nodes.remove(&id);
    }

    pub fn get_node(&self, key: &str) -> Option<&NodeInfo> {
        if self.vnodes.is_empty() {
            return None;
        }
        let h = hash_string(key);
        let mut iter = self.vnodes.range(h..);
        let (_, node_id) = iter.next()?;
        self.nodes.get(node_id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn vnode_count(&self) -> usize {
        self.vnodes.len()
    }
}

fn hash_string(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    std::hash::Hasher::finish(&hasher)
}

fn hash_node_vnode(id: &NodeId, vnode: usize) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    vnode.hash(&mut hasher);
    std::hash::Hasher::finish(&hasher)
}

/// Gossip-based membership state.
pub struct GossipState {
    local_id: NodeId,
    #[allow(dead_code)]
    #[allow(dead_code)]
    local_addr: String,
    members: RwLock<HashMap<NodeId, NodeInfo>>,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    last_tick: RwLock<Instant>,
}

impl GossipState {
    pub fn new(local_id: NodeId, local_addr: String, interval_ms: u64, timeout_ms: u64) -> Self {
        let mut members = HashMap::new();
        members.insert(
            local_id,
            NodeInfo {
                id: local_id,
                addr: local_addr.clone(),
                last_heartbeat: now_millis(),
                is_local: true,
            },
        );
        Self {
            local_id,
            local_addr,
            members: RwLock::new(members),
            heartbeat_interval: Duration::from_millis(interval_ms),
            heartbeat_timeout: Duration::from_millis(timeout_ms),
            last_tick: RwLock::new(Instant::now()),
        }
    }

    pub fn merge(&self, other: &HashMap<NodeId, NodeInfo>) {
        let mut members = self.members.write();
        for (id, info) in other {
            if *id == self.local_id {
                continue;
            }
            match members.get_mut(id) {
                Some(existing) if info.last_heartbeat > existing.last_heartbeat => {
                    existing.last_heartbeat = info.last_heartbeat;
                    existing.addr = info.addr.clone();
                }
                None => {
                    members.insert(*id, info.clone());
                }
                _ => {}
            }
        }
    }

    pub fn is_healthy(&self, node_id: NodeId) -> bool {
        let members = self.members.read();
        let Some(info) = members.get(&node_id) else {
            return false;
        };
        let elapsed = now_millis().saturating_sub(info.last_heartbeat);
        elapsed < self.heartbeat_timeout.as_millis() as u64
    }

    pub fn healthy_members(&self) -> Vec<NodeInfo> {
        let members = self.members.read();
        let now = now_millis();
        members
            .values()
            .filter(|m| {
                let elapsed = now.saturating_sub(m.last_heartbeat);
                elapsed < self.heartbeat_timeout.as_millis() as u64
            })
            .cloned()
            .collect()
    }

    pub fn all_members(&self) -> HashMap<NodeId, NodeInfo> {
        self.members.read().clone()
    }

    pub fn local_id(&self) -> NodeId {
        self.local_id
    }

    pub fn touch_heartbeat(&self) {
        let mut members = self.members.write();
        if let Some(local) = members.get_mut(&self.local_id) {
            local.last_heartbeat = now_millis();
        }
        *self.last_tick.write() = Instant::now();
    }

    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    pub fn remove_stale(&self) -> Vec<NodeId> {
        let mut members = self.members.write();
        let now = now_millis();
        let stale: Vec<NodeId> = members
            .iter()
            .filter(|(id, m)| {
                **id != self.local_id
                    && now.saturating_sub(m.last_heartbeat) >= self.heartbeat_timeout.as_millis() as u64
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &stale {
            members.remove(id);
        }
        stale
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Distributed bus wrapper that can optionally forward to remote nodes.
pub struct DistributedBus {
    local_tx: crate::engine::bus::EventSender,
    cluster: Option<Arc<ClusterHandle>>,
}

pub struct ClusterHandle {
    pub ring: RwLock<HashRing>,
    pub gossip: Arc<GossipState>,
    pub config: ClusterConfig,
}

impl DistributedBus {
    pub fn new(local_tx: crate::engine::bus::EventSender, cluster: Option<Arc<ClusterHandle>>) -> Self {
        Self { local_tx, cluster }
    }

    pub fn publish(&self, event: crate::engine::bus::EngineEvent) {
        let _ = self.local_tx.send(event.clone());
        if let Some(cluster) = &self.cluster {
            if cluster.config.enabled {
                cluster.gossip.touch_heartbeat();
            }
        }
    }

    pub fn is_clustered(&self) -> bool {
        self.cluster.as_ref().map_or(false, |c| c.config.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_config_default_and_env() {
        let cfg = ClusterConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.vnode_count, 128);
        assert_eq!(cfg.heartbeat_interval_ms, 1000);
    }

    #[test]
    fn hash_ring_basic_operations() {
        let mut ring = HashRing::new(16);
        assert_eq!(ring.node_count(), 0);

        let n1 = NodeInfo {
            id: NodeId::new_v4(),
            addr: "127.0.0.1:8001".to_string(),
            last_heartbeat: 0,
            is_local: false,
        };
        let n2 = NodeInfo {
            id: NodeId::new_v4(),
            addr: "127.0.0.1:8002".to_string(),
            last_heartbeat: 0,
            is_local: false,
        };

        ring.add_node(n1.clone());
        assert_eq!(ring.node_count(), 1);
        assert_eq!(ring.vnode_count(), 16);

        ring.add_node(n2.clone());
        assert_eq!(ring.node_count(), 2);
        assert_eq!(ring.vnode_count(), 32);

        let target = ring.get_node("user_42");
        assert!(target.is_some());

        ring.remove_node(n1.id);
        assert_eq!(ring.node_count(), 1);
        assert_eq!(ring.vnode_count(), 16);
    }

    #[test]
    fn hash_ring_routes_consistently() {
        let mut ring = HashRing::new(64);
        let n1 = NodeInfo {
            id: NodeId::new_v4(),
            addr: "127.0.0.1:8001".to_string(),
            last_heartbeat: 0,
            is_local: false,
        };
        ring.add_node(n1);

        let first = ring.get_node("user_stable");
        assert!(first.is_some());
        let second = ring.get_node("user_stable");
        assert_eq!(first.unwrap().id, second.unwrap().id);
    }

    #[test]
    fn gossip_merge_and_health() {
        let id_a = NodeId::new_v4();
        let id_b = NodeId::new_v4();
        let gossip = GossipState::new(id_a, "127.0.0.1:8001".to_string(), 100, 500);

        let mut remote = HashMap::new();
        remote.insert(
            id_b,
            NodeInfo {
                id: id_b,
                addr: "127.0.0.1:8002".to_string(),
                last_heartbeat: now_millis(),
                is_local: false,
            },
        );

        gossip.merge(&remote);
        assert!(gossip.is_healthy(id_b));
        assert!(gossip.is_healthy(id_a));

        let stale = GossipState::new(id_b, "127.0.0.1:8002".to_string(), 100, 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!stale.is_healthy(id_b));
    }

    #[test]
    fn gossip_remove_stale() {
        let id_a = NodeId::new_v4();
        let id_b = NodeId::new_v4();
        let gossip = GossipState::new(id_a, "127.0.0.1:8001".to_string(), 100, 1);

        let mut remote = HashMap::new();
        remote.insert(
            id_b,
            NodeInfo {
                id: id_b,
                addr: "127.0.0.1:8002".to_string(),
                last_heartbeat: now_millis(),
                is_local: false,
            },
        );
        gossip.merge(&remote);
        assert_eq!(gossip.all_members().len(), 2);

        std::thread::sleep(std::time::Duration::from_millis(20));
        let removed = gossip.remove_stale();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0], id_b);
        assert_eq!(gossip.all_members().len(), 1);
    }

    #[test]
    fn distributed_bus_clustered_flag() {
        let (tx, _) = tokio::sync::broadcast::channel(4);
        let bus = DistributedBus::new(tx.clone(), None);
        assert!(!bus.is_clustered());

        let id = NodeId::new_v4();
        let gossip = Arc::new(GossipState::new(id, "127.0.0.1:8001".to_string(), 100, 500));
        let handle = Arc::new(ClusterHandle {
            ring: RwLock::new(HashRing::new(16)),
            gossip,
            config: ClusterConfig {
                enabled: true,
                ..ClusterConfig::default()
            },
        });
        let bus2 = DistributedBus::new(tx, Some(handle));
        assert!(bus2.is_clustered());
    }
}
