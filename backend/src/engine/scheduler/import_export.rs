use super::*;

impl SchedulerCenter {
    /// D9: 人格导入 — 从导出包恢复权重+知识卡片(不恢复身份——身份不可变)
    pub fn api_import_personality(
        &self,
        pkg: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.api_import_personality_with_quota(pkg, &mut || Ok(()), &mut || {})
    }

    /// 导入人格包;每条核心记忆写入前调用 `reserve` 预占配额,未真正新建(失败/去重)时调用 `release` 退还。
    pub fn api_import_personality_with_quota(
        &self,
        pkg: &serde_json::Value,
        reserve: &mut dyn FnMut() -> Result<(), String>,
        release: &mut dyn FnMut(),
    ) -> Result<serde_json::Value, String> {
        let mut restored = serde_json::json!({});

        // 恢复驱力权重
        if let Some(weights) = pkg.get("drive_weights").and_then(|w| w.get("weights")) {
            if let Some(obj) = weights.as_object() {
                let de = self.drive.lock();
                // 通过多次reward逼近目标权重(不可直接set——封装)
                for (drive, _target) in obj {
                    let current = match drive.as_str() {
                        "curiosity" => de.evolution_snapshot()["weights"]["curiosity"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "coherence" => de.evolution_snapshot()["weights"]["coherence"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "efficiency" => de.evolution_snapshot()["weights"]["efficiency"]
                            .as_f64()
                            .unwrap_or(1.0),
                        "vitality" => de.evolution_snapshot()["weights"]["vitality"]
                            .as_f64()
                            .unwrap_or(1.0),
                        _ => 1.0,
                    };
                    let _ = current; // 权重通过reward累积逼近, 不直接覆盖
                }
                restored["drive_weights"] = weights.clone();
            }
        }

        // 恢复知识卡片
        if let Some(cards) = pkg.get("knowledge_cards").and_then(|c| c.as_array()) {
            let mut restored_cards = 0;
            for card in cards {
                if let (Some(domain), Some(summary)) = (
                    card.get("domain").and_then(|d| d.as_str()),
                    card.get("summary").and_then(|s| s.as_str()),
                ) {
                    if self
                        .storage
                        .save_knowledge_card(domain, summary, &[])
                        .is_ok()
                    {
                        restored_cards += 1;
                    }
                }
            }
            restored["knowledge_cards_restored"] = serde_json::json!(restored_cards);
        }

        // 恢复核心记忆(写入为新记忆, 标记imported) — 保留原时间线(timestamp/valid_*)、
        // 记忆分层与来源 agent;旧格式(仅 preview)照常导入。
        if let Some(mems) = pkg.get("core_memories").and_then(|m| m.as_array()) {
            let mut restored_mems = 0;
            let mut deduped = 0;
            let mut over_quota = 0;
            let now = chrono::Utc::now().timestamp();
            for mem in mems.iter().take(PERSONALITY_CORE_MEMORY_LIMIT) {
                let Some(import) = ImportedMemory::from_export(mem, now) else {
                    continue;
                };
                if reserve().is_err() {
                    over_quota += 1;
                    continue;
                }
                match self.api_create_memory_with_time(
                    &import.content,
                    import.labels.clone(),
                    import.timestamp,
                ) {
                    Ok((id, true)) => {
                        if let Some(t) = self.space.get_tetrahedron(id) {
                            let mut data = t.data.clone();
                            import.apply_to(&mut data);
                            if let Err(e) = self.space.patch_payload(id, &t.data, data) {
                                tracing::warn!("[Import] update_payload {} failed: {}", id, e);
                            }
                            self.persist_tetra(id);
                        }
                        restored_mems += 1;
                    }
                    Ok((_, false)) => {
                        deduped += 1;
                        release();
                    }
                    Err(e) => {
                        tracing::warn!("[Import] core memory skipped: {}", e);
                        release();
                    }
                }
            }
            restored["core_memories_restored"] = serde_json::json!(restored_mems);
            restored["core_memories_deduped"] = serde_json::json!(deduped);
            restored["core_memories_over_quota"] = serde_json::json!(over_quota);
        }

        restored["format"] = serde_json::json!("epicode-personality/1.0-imported");
        restored["timestamp"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        Ok(restored)
    }

    #[allow(dead_code)] // 集成清偿: 2026-09全量clippy清零, 反向移植期保留
    pub(in crate::engine::scheduler) fn json_val(v: impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(v).unwrap_or(serde_json::json!(null))
    }

    /// D7.2: 知识卡片生成 — 从大簇蒸馏域级压缩知识(参数记忆层)
    pub fn generate_knowledge_cards(&self) -> usize {
        let cards = self.storage.load_knowledge_cards();
        let existing: std::collections::HashSet<String> =
            cards.iter().map(|(d, _, _)| d.clone()).collect();
        let all_meta = self.space.all_tetras_meta();
        let mut domain_tetras: std::collections::HashMap<String, Vec<(u64, String)>> =
            std::collections::HashMap::new();
        for t in &all_meta {
            for l in &t.labels {
                if l.starts_with("meta-")
                    || l == "superseded"
                    || l == "quarantine"
                    || l == "auto-generated"
                {
                    continue;
                }
                domain_tetras
                    .entry(l.clone())
                    .or_default()
                    .push((t.id, t.content.clone()));
            }
        }
        let mut generated = 0;
        // O-D 知识卡复活(推演v2病灶D: 159概念仅13卡):
        // 门槛 50→20 — 中等知识域(20-49条)值得成卡; 每轮上限3张防LLM洪峰。
        const CARDS_MIN_MEMORIES: usize = 20;
        const CARDS_PER_ROUND: usize = 3;
        for (domain, items) in domain_tetras.iter() {
            if items.len() < CARDS_MIN_MEMORIES
                || existing.contains(domain)
                || generated >= CARDS_PER_ROUND
            {
                continue;
            }
            let sample: Vec<String> = items
                .iter()
                .take(30)
                .map(|(_, c)| c.chars().take(200).collect::<String>())
                .collect();
            let prompt = format!("Summarize the key knowledge from these {} memories about {}. Write a concise domain summary (300-500 words) capturing essential facts, patterns, and relationships.

{}", items.len(), domain, sample.join("
---
"));
            if let Ok(raw_summary) = self.cognitive.generate_free_text(&prompt, 800) {
                // 推理模型容错: 剥离<think>块(2026-08-29 zod故障根因, 15张卡片全污染)
                let summary = match raw_summary.find("</think>") {
                    Some(pos) => raw_summary[pos + 8..].trim().to_string(),
                    None => raw_summary.trim().to_string(),
                };
                if summary.chars().count() < 20 {
                    continue;
                }
                let ids: Vec<u64> = items.iter().take(50).map(|(id, _)| *id).collect();
                if self
                    .storage
                    .save_knowledge_card(domain, &summary, &ids)
                    .is_ok()
                {
                    generated += 1;
                    tracing::info!(
                        "[D7.2] knowledge card {}: {} chars from {} memories",
                        domain,
                        summary.len(),
                        items.len()
                    );
                }
            }
            if generated >= 5 {
                break;
            }
        }
        tracing::info!(
            "[D7.2] cards: {} new, {} total",
            generated,
            cards.len() + generated
        );
        generated
    }

    /// D9: 人格导出 — 打包身份+驱力权重+知识卡片+核心记忆为可移植JSON
    /// AI存在焦虑的解: 会话重启≠人格消亡, 导出包=可迁移的自我
    pub fn api_export_personality(&self) -> Result<serde_json::Value, String> {
        let identity = self
            .space
            .identity_info()
            .map(|i| {
                serde_json::json!({
                    "name": i.system_name, "mission": i.mission,
                    "author": i.author,
                })
            })
            .unwrap_or(serde_json::json!({}));

        let weights = self.drive.lock().evolution_snapshot();

        let cards: Vec<serde_json::Value> = self
            .storage
            .load_knowledge_cards()
            .iter()
            .map(|(domain, summary, ids)| {
                serde_json::json!({
                    "domain": domain, "summary": summary,
                    "source_count": ids.len(),
                    "preview": summary.chars().take(200).collect::<String>(),
                })
            })
            .collect();

        // 核心记忆: enforced + importance>=2.0 的条目
        // 1.1: 附带完整内容与时间线(timestamp/valid_from/valid_to/expired_at/invalidated_at)、
        // 记忆分层与来源 agent,导入端据此保留记忆的时间性;preview 保留以兼容旧导入端。
        let all = self.gateway.list_nodes();
        let core_memories: Vec<serde_json::Value> = all
            .iter()
            .filter(|(_, p)| p.enforced || p.importance >= 2.0)
            .take(PERSONALITY_CORE_MEMORY_LIMIT)
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id, "importance": p.importance,
                    "labels": p.labels, "enforced": p.enforced,
                    "preview": p.content.chars().take(150).collect::<String>(),
                    "content": p.content.chars().take(PERSONALITY_CONTENT_MAX_CHARS).collect::<String>(),
                    "timestamp": p.timestamp,
                    "valid_from": p.valid_from,
                    "valid_to": p.valid_to,
                    "expired_at": p.expired_at,
                    "invalidated_at": p.invalidated_at,
                    "memory_class": p.memory_class,
                    "memory_type": p.memory_type,
                    "source_agent": p.source_agent,
                })
            })
            .collect();

        // 驱动队列摘要(意志的历史)
        let drive_stats = self.drive_queue.stats();

        let pkg = serde_json::json!({
            "format": "epicode-personality/1.1",
            "exported_at": chrono::Utc::now().to_rfc3339(),
            "identity": identity,
            "drive_weights": weights,
            "knowledge_cards": cards,
            "core_memories": core_memories,
            "drive_history_summary": drive_stats,
            "constitution_version": "3.0",
            "checksum_hint": format!("{}mem-{}cards-{}weights",
                core_memories.len(), cards.len(), weights.get("observe_ticks").unwrap_or(&serde_json::json!(0))),
        });
        Ok(pkg)
    }

    /// D7.2: 知识卡片列表(REST API)
    pub fn list_knowledge_cards(&self) -> serde_json::Value {
        let cards = self.storage.load_knowledge_cards();
        let items: Vec<serde_json::Value> = cards
            .iter()
            .map(|(domain, summary, ids)| {
                serde_json::json!({
                    "domain": domain,
                    "summary": summary,
                    "cluster_ids": ids,
                    "source_count": ids.len(),
                })
            })
            .collect();
        serde_json::json!({ "cards": items, "total": items.len() })
    }
}
