use std::sync::Arc;

use crate::domain::space::Space;

use super::dynamics;
use super::energy::EnergyCenter;
use super::knowledge::KnowledgeGraph;
use super::security::SecurityGuard;

pub struct ToolContext {
    pub space: Arc<Space>,
    pub energy: Arc<EnergyCenter>,
    pub knowledge: Arc<KnowledgeGraph>,
    pub security: Arc<SecurityGuard>,
    pub max_energy: f64,
}

impl ToolContext {
    pub fn new(
        space: Arc<Space>,
        energy: Arc<EnergyCenter>,
        knowledge: Arc<KnowledgeGraph>,
        security: Arc<SecurityGuard>,
        max_energy: f64,
    ) -> Self {
        Self {
            space,
            energy,
            knowledge,
            security,
            max_energy,
        }
    }
}

pub struct ToolRegistry {
    ctx: Arc<ToolContext>,
}

impl super::cognitive::ToolProvider for ToolRegistry {
    fn execute_tool(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
        self.execute(name, args)
    }

    fn definitions(&self) -> Vec<serde_json::Value> {
        Self::tool_definitions()
    }
}

impl ToolRegistry {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }

    pub fn tool_definitions() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "query_memory",
                    "strict": true,
                    "description": "查看特定记忆的完整内容、标签、邻居关系、质量分数。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "integer", "description": "记忆四面体的ID"}
                        },
                        "required": ["id"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "cluster_detail",
                    "strict": true,
                    "description": "深入探查某个簇的所有成员、标签分布、熵值。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "index": {"type": "integer", "description": "簇索引号"}
                        },
                        "required": ["index"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "search_memories",
                    "strict": true,
                    "description": "关键词搜索记忆。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string", "description": "搜索查询文本"},
                            "limit": {"type": "integer", "description": "返回数量上限，默认5"}
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "cluster_similarity",
                    "strict": true,
                    "description": "计算两个簇之间的标签相似度。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cluster_a": {"type": "integer", "description": "第一个簇的索引"},
                            "cluster_b": {"type": "integer", "description": "第二个簇的索引"}
                        },
                        "required": ["cluster_a", "cluster_b"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "check_operation",
                    "strict": true,
                    "description": "预检查某个操作是否可行。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "operation": {"type": "string", "description": "操作类型", "enum": ["fission", "fuse", "link", "dream"]},
                            "params": {"type": "string", "description": "操作参数，JSON格式"}
                        },
                        "required": ["operation"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_by_label",
                    "strict": true,
                    "description": "按标签列出所有记忆。",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "label": {"type": "string", "description": "要查找的标签名"}
                        },
                        "required": ["label"],
                        "additionalProperties": false
                    }
                }
            }),
        ]
    }

    pub fn execute(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
        match name {
            "query_memory" => self.query_memory(args),
            "cluster_detail" => self.cluster_detail(args),
            "search_memories" => self.search_memories(args),
            "cluster_similarity" => self.cluster_similarity(args),
            "check_operation" => self.check_operation(args),
            "list_by_label" => self.list_by_label(args),
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    fn query_memory(&self, args: &serde_json::Value) -> Result<String, String> {
        let id = args["id"].as_u64().ok_or("missing id")?;
        let t = self
            .ctx
            .space
            .get_tetrahedron(id)
            .ok_or(format!("tetra {id} not found"))?;
        let neighbors = self.ctx.knowledge.query_relations(id);
        Ok(serde_json::json!({
            "id": t.id,
            "content": t.data.content,
            "labels": t.data.labels,
            "mass": t.mass,
            "position": [t.core.x, t.core.y, t.core.z],
            "aliases": t.data.aliases.iter().take(5).collect::<Vec<_>>(),
            "neighbors": neighbors.iter().take(5)
                .map(|(nid, rel, strength)| serde_json::json!({"id": nid, "relation": format!("{:?}", rel), "strength": format!("{:.2}", strength)}))
                .collect::<Vec<_>>()
        }).to_string())
    }

    fn cluster_detail(&self, args: &serde_json::Value) -> Result<String, String> {
        let idx = args["index"].as_u64().ok_or("missing index")? as usize;
        let clusters = self.ctx.space.find_clusters();
        let cluster = clusters.get(idx).ok_or(format!(
            "cluster {} not found (total: {})",
            idx,
            clusters.len()
        ))?;
        let mut members = Vec::new();
        let mut label_counts = std::collections::HashMap::new();
        for &id in &cluster.tetra_ids {
            if let Some(t) = self.ctx.space.get_tetrahedron(id) {
                members.push(serde_json::json!({
                    "id": t.id,
                    "content": t.data.content.chars().take(80).collect::<String>(),
                    "labels": t.data.labels,
                    "mass": format!("{:.2}", t.mass)
                }));
                for l in &t.data.labels {
                    *label_counts.entry(l.clone()).or_insert(0usize) += 1;
                }
            }
        }
        let entropy = dynamics::compute_entropy(&self.ctx.space, cluster);
        Ok(serde_json::json!({
            "index": idx,
            "size": cluster.tetra_ids.len(),
            "entropy": format!("{:.3}", entropy),
            "labels": label_counts,
            "members": members
        })
        .to_string())
    }

    fn search_memories(&self, args: &serde_json::Value) -> Result<String, String> {
        let query = args["query"].as_str().ok_or("missing query")?;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let all = self.ctx.space.all_tetrahedrons();
        let mut scored: Vec<(u64, f64, String, Vec<String>)> = all
            .into_iter()
            .filter(|t| {
                !t.data
                    .labels
                    .iter()
                    .any(|l| l.starts_with("meta-") || l.starts_with("bridge"))
            })
            .map(|t| {
                let content_lower = t.data.content.to_lowercase();
                let label_text = t.data.labels.join(" ").to_lowercase();
                let alias_text = t.data.aliases.join(" ").to_lowercase();
                let searchable = format!("{content_lower} {label_text} {alias_text}");
                let sim = if query_words.is_empty() {
                    0.0
                } else {
                    let matched = query_words
                        .iter()
                        .filter(|w| searchable.contains(*w))
                        .count();
                    matched as f64 / query_words.len() as f64
                };
                (
                    t.id,
                    sim,
                    t.data.content.chars().take(80).collect::<String>(),
                    t.data.labels.clone(),
                )
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        let results: Vec<serde_json::Value> = scored.into_iter()
            .map(|(id, sim, content, labels)| {
                serde_json::json!({"id": id, "similarity": format!("{:.3}", sim), "content": content, "labels": labels})
            })
            .collect();
        Ok(
            serde_json::json!({"query": query, "count": results.len(), "results": results})
                .to_string(),
        )
    }

    fn cluster_similarity(&self, args: &serde_json::Value) -> Result<String, String> {
        let a = args["cluster_a"].as_u64().ok_or("missing cluster_a")? as usize;
        let b = args["cluster_b"].as_u64().ok_or("missing cluster_b")? as usize;
        let clusters = self.ctx.space.find_clusters();
        let ca = clusters.get(a).ok_or(format!("cluster {a} not found"))?;
        let cb = clusters.get(b).ok_or(format!("cluster {b} not found"))?;

        let labels_a: std::collections::HashSet<String> = ca
            .tetra_ids
            .iter()
            .filter_map(|id| self.ctx.space.get_tetrahedron(*id))
            .flat_map(|t| t.data.labels.clone())
            .collect();
        let labels_b: std::collections::HashSet<String> = cb
            .tetra_ids
            .iter()
            .filter_map(|id| self.ctx.space.get_tetrahedron(*id))
            .flat_map(|t| t.data.labels.clone())
            .collect();

        let intersection = labels_a.intersection(&labels_b).count();
        let union = labels_a.union(&labels_b).count();
        let avg = if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        };

        Ok(serde_json::json!({
            "cluster_a": a, "cluster_b": b,
            "avg_similarity": format!("{:.3}", avg),
            "labels_a": labels_a.len(),
            "labels_b": labels_b.len(),
            "shared_labels": intersection,
        })
        .to_string())
    }

    fn check_operation(&self, args: &serde_json::Value) -> Result<String, String> {
        let op = args["operation"].as_str().ok_or("missing operation")?;
        let energy = self.ctx.energy.available();
        let mut checks = Vec::new();
        let mut feasible = true;

        match op {
            "fission" => {
                let params_idx = args["params"]
                    .as_str()
                    .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                    .and_then(|v| v["cluster_index"].as_u64());
                let clusters = self.ctx.space.find_clusters();
                if let Some(idx) = params_idx {
                    if let Some(c) = clusters.get(idx as usize) {
                        let entropy = dynamics::compute_entropy(&self.ctx.space, c);
                        checks.push(format!("entropy={entropy:.3} (need>0.3)"));
                        if entropy < 0.3 {
                            feasible = false;
                        }
                        checks.push(format!("size={} (need>=6)", c.tetra_ids.len()));
                        if c.tetra_ids.len() < 6 {
                            feasible = false;
                        }
                    } else {
                        checks.push(format!("cluster {idx} not found"));
                        feasible = false;
                    }
                } else {
                    checks.push("no cluster_index specified".to_string());
                }
                checks.push("cooldown=10ticks (auto_fission manages this)".to_string());
                checks.push("energy_cost=8".to_string());
                if energy < 8.0 {
                    feasible = false;
                }
            }
            "fuse" => {
                checks.push("energy_cost=8".to_string());
                if energy < 8.0 {
                    feasible = false;
                }
                checks.push("cluster_a!=cluster_b (verify before calling)".to_string());
            }
            "link" => {
                checks.push("energy_cost=0 (free)".to_string());
            }
            "dream" => {
                checks.push("energy_cost=15".to_string());
                if energy < 15.0 {
                    feasible = false;
                }
            }
            _ => {
                checks.push(format!("unknown operation: {op}"));
                feasible = false;
            }
        }
        checks.push(format!(
            "energy_available={:.0}/{}",
            energy, self.ctx.max_energy
        ));

        Ok(
            serde_json::json!({"operation": op, "checks": checks, "feasible": feasible})
                .to_string(),
        )
    }

    fn list_by_label(&self, args: &serde_json::Value) -> Result<String, String> {
        let label = args["label"].as_str().ok_or("missing label")?;
        let tetras = self.ctx.space.all_tetrahedrons();
        let matches: Vec<serde_json::Value> = tetras
            .iter()
            .filter(|t| t.data.labels.iter().any(|l| l == label))
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "content": t.data.content.chars().take(80).collect::<String>(),
                    "labels": t.data.labels
                })
            })
            .collect();
        Ok(
            serde_json::json!({"label": label, "count": matches.len(), "memories": matches})
                .to_string(),
        )
    }
}
