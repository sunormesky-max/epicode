use super::super::*;

impl McpHandler {
    pub(in crate::engine::mcp) fn tool_space_stats(&self) -> serde_json::Value {
        let stats = self.engine.scheduler.api_stats();
        let (ports_assigned, ports_free) = self.engine.space.port_stats();
        // M5修复：用 try_cluster_count 避免高频诊断工具触发 O(N) 全量聚类
        let cluster_count = self
            .engine
            .space
            .try_cluster_count()
            .unwrap_or_else(|| self.engine.scheduler.find_clusters_cached().len());
        // M5修复：cluster_distribution 用 stats.clusters（已缓存），不再做全量遍历
        let data = serde_json::json!({
            "memories": stats.tetra_count,
            "vertices": stats.vertex_count,
            "clusters": stats.clusters,
            "energy": (stats.energy * 100.0).round() / 100.0,
            "cluster_distribution": {
                "count": cluster_count,
            },
            "capacity": {
                "ports_assigned": ports_assigned,
                "ports_free": ports_free,
                "ports_total": ports_assigned + ports_free,
                "ratio_per_cluster": if stats.clusters > 0 {
                    (ports_assigned as f64 / stats.clusters as f64 * 100.0).round() / 100.0
                } else { 0.0 },
            },
        });
        self.smrp_ok("space_stats", data)
    }

    pub(in crate::engine::mcp) fn tool_knowledge_relations(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let id = match args["id"].as_u64() {
            Some(id) => id,
            None => return self.smrp_err("knowledge_relations", 400, "id is required"),
        };
        let inline = args["inline_content"].as_bool().unwrap_or(false);
        let rels = self.engine.scheduler.api_get_relations(id);
        // graph_view：度数/类型分布/最强边/双向边（SMRP §7.1）
        let mut type_dist: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        let mut strongest: Option<serde_json::Value> = None;
        let mut targets: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let items: Vec<serde_json::Value> = rels.iter().map(|(target, rel_type, strength)| {
            *type_dist.entry(rel_type.as_str()).or_insert(0) += 1;
            targets.insert(*target);
            if strongest.as_ref().and_then(|s| s["strength"].as_f64()).is_none_or(|s| *strength > s) {
                strongest = Some(serde_json::json!({"target": target, "type": rel_type, "strength": (*strength * 100.0).round() / 100.0}));
            }
            let mut item = serde_json::json!({
                "target": target,
                "type": rel_type,
                "strength": (*strength * 100.0).round() / 100.0,
            });
            if inline {
                if let Some(payload) = self.engine.scheduler.api_get_node(*target) {
                    item["target_content"] = serde_json::Value::String(payload.content.chars().take(200).collect());
                    item["target_labels"] = serde_json::Value::Array(payload.labels.into_iter().map(serde_json::Value::String).collect());
                }
            }
            item
        }).collect();
        // 双向边：target 也指向 id
        let mutual: Vec<u64> = targets
            .iter()
            .filter(|t| {
                self.engine
                    .scheduler
                    .api_get_relations(**t)
                    .iter()
                    .any(|(tgt, _, _)| tgt == &id)
            })
            .copied()
            .collect();
        let data = serde_json::json!({
            "id": id,
            "relations": items,
            "count": items.len(),
            "graph_view": {
                "degree": items.len(),
                "type_distribution": type_dist,
                "strongest": strongest,
                "mutual": mutual,
            },
        });
        self.smrp_ok("knowledge_relations", data)
    }

    pub(in crate::engine::mcp) fn tool_concepts(&self) -> serde_json::Value {
        let concepts = self.engine.scheduler.api_get_concepts();
        let label_idx = self.engine.scheduler().gateway_handle();
        let mut auto_count = 0u32;
        let items: Vec<serde_json::Value> = concepts.into_iter().map(|(label, count)| {
            let is_auto = label.starts_with("concept_");
            if is_auto { auto_count += 1; }
            // 取该标签下前 3 个样本 id
            let sample_ids: Vec<u64> = label_idx.list_by_labels(&[label.as_str()], 3)
                .into_iter().map(|(tid, _)| tid).collect();
            serde_json::json!({"label": label, "member_count": count, "sample_ids": sample_ids, "is_auto_extracted": is_auto})
        }).collect();
        let data = serde_json::json!({
            "concepts": items,
            "count": items.len(),
            "auto_discovered_count": auto_count,
        });
        self.smrp_ok("concepts", data)
    }

    pub(in crate::engine::mcp) fn tool_kg_quality(
        &self,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        let sample_size = args["sample_size"]
            .as_u64()
            .map(|v| v.min(200) as usize)
            .unwrap_or(50);

        let tetras = self.engine.space().all_tetrahedrons();
        let total = tetras.len();
        if total == 0 {
            return self.smrp_err("kg_quality", 404, "no memories found");
        }

        let step = (total / sample_size).max(1);
        let sampled: Vec<_> = tetras.iter().step_by(step).take(sample_size).collect();

        let mut relation_counts: Vec<usize> = Vec::new();
        let mut orphan_count = 0usize;
        let mut all_strengths: Vec<f64> = Vec::new();
        let mut total_relations = 0usize;

        for t in &sampled {
            let rels = self.engine.scheduler().api_get_relations(t.id);
            let count = rels.len();
            relation_counts.push(count);
            total_relations += count;
            if count == 0 {
                orphan_count += 1;
            }
            for (_, _, strength) in &rels {
                all_strengths.push(*strength);
            }
        }

        let sampled_n = sampled.len();
        let avg_rels = if sampled_n > 0 {
            total_relations as f64 / sampled_n as f64
        } else {
            0.0
        };
        let orphan_rate = if sampled_n > 0 {
            orphan_count as f64 / sampled_n as f64 * 100.0
        } else {
            0.0
        };

        let avg_strength = if !all_strengths.is_empty() {
            all_strengths.iter().sum::<f64>() / all_strengths.len() as f64
        } else {
            0.0
        };

        let strong_rels = all_strengths.iter().filter(|&&s| s >= 0.5).count();
        let weak_rels = all_strengths.iter().filter(|&&s| s < 0.2).count();
        let mid_rels = all_strengths.len() - strong_rels - weak_rels;

        let clusters = self.engine.space().find_clusters();
        let cluster_sizes: Vec<usize> = clusters.iter().map(|c| c.tetra_ids.len()).collect();
        let largest_cluster = cluster_sizes.iter().copied().max().unwrap_or(0);
        let avg_cluster_size = if !cluster_sizes.is_empty() {
            cluster_sizes.iter().sum::<usize>() as f64 / cluster_sizes.len() as f64
        } else {
            0.0
        };

        let max_rel = relation_counts.iter().copied().max().unwrap_or(0);
        let min_rel = relation_counts.iter().copied().min().unwrap_or(0);

        let density_score = if total > 0 {
            (total_relations as f64 / (sampled_n as f64 * 20.0)).min(1.0) * 100.0
        } else {
            0.0
        };

        self.smrp_ok("kg_quality", serde_json::json!({
            "total_memories": total,
            "sampled": sampled_n,
            "total_clusters": clusters.len(),
            "avg_cluster_size": (avg_cluster_size * 10.0).round() / 10.0,
            "largest_cluster": largest_cluster,
            "relation_density": {
                "avg_per_memory": (avg_rels * 10.0).round() / 10.0,
                "max": max_rel,
                "min": min_rel,
                "total_sampled": total_relations,
            },
            "orphan_rate_pct": (orphan_rate * 10.0).round() / 10.0,
            "strength_distribution": {
                "strong_ge_0.5": strong_rels,
                "medium": mid_rels,
                "weak_lt_0.2": weak_rels,
                "avg_strength": (avg_strength * 1000.0).round() / 1000.0,
            },
            "density_score": (density_score * 10.0).round() / 10.0,
            "assessment": if orphan_rate > 50.0 {
                "poor — high orphan rate, run dream_cycle to build connections"
            } else if avg_rels < 5.0 {
                "developing — relation density below optimal, pulses are still building connections"
            } else if orphan_rate < 10.0 && avg_rels > 15.0 {
                "excellent — dense interconnection with low orphan rate"
            } else {
                "healthy — reasonable relation density and connectivity"
            },
        }))
    }
}
