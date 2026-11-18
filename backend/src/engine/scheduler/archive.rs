use super::*;

impl SchedulerCenter {
    /// 一次性迁移：从标签字符串方案迁移到 archive_nodes 表（幂等，每次 tree 查询时执行）
    /// 扫描所有记忆，把有 archive 标签的、或文档特征孤儿（# 开头+长内容+文档类标签）的，INSERT OR IGNORE 到表
    pub(in crate::engine::scheduler) fn archive_migrate_labels_to_table(&self) {
        let meta = self.space.all_tetras_meta();
        let meta_by_id: HashMap<u64, &crate::domain::tetra::TetraMeta> =
            meta.iter().map(|t| (t.id, t)).collect();

        // 1. 有 archive 标签的记忆 → 迁移到表
        for t in &meta {
            let has_archive = t
                .labels
                .iter()
                .any(|l| l == "archive" || l.starts_with("archive."));
            let is_archived = t.labels.iter().any(|l| l == "archived");
            if !has_archive || is_archived {
                continue;
            }

            // 解析 node_type
            let node_type = if t.labels.iter().any(|l| l == "archive")
                && !t.labels.iter().any(|l| l.starts_with("archive."))
            {
                "root".to_string()
            } else {
                t.labels
                    .iter()
                    .find_map(|l| l.strip_prefix("archive."))
                    .unwrap_or("doc")
                    .to_string()
            };
            // 解析 parent_id
            let parent_id: Option<i64> = t
                .labels
                .iter()
                .find_map(|l| {
                    l.strip_prefix("parent:")
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .filter(|pid| meta_by_id.contains_key(&(*pid as u64)));
            // 解析 category
            let category = t
                .labels
                .iter()
                .find_map(|l| l.strip_prefix("category:"))
                .unwrap_or("")
                .to_string();

            let _ = self
                .storage
                .archive_upsert_node(t.id as i64, parent_id, &node_type, &category);
        }

        // 2. 文档特征孤儿收编：以 # 开头 + 长内容(>200) + 文档类标签 但不在表中的记忆
        let doc_labels = [
            "documentation",
            "system",
            "architecture",
            "devops",
            "infrastructure",
            "security",
            "ai",
            "ml",
            "database",
            "storage",
            "networking",
            "protocol",
            "biology",
            "life",
        ];
        for t in &meta {
            if self.storage.archive_node_exists(t.id as i64) {
                continue;
            }
            let is_doc = t.content.starts_with("# ") && t.content.len() > 200;
            let has_doc_label = t.labels.iter().any(|l| doc_labels.contains(&l.as_str()));
            if is_doc && has_doc_label {
                // 解析 parent_id（如果有）
                let parent_id: Option<i64> = t
                    .labels
                    .iter()
                    .find_map(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<i64>().ok())
                    })
                    .filter(|pid| self.storage.archive_node_exists(*pid));
                let category = t
                    .labels
                    .iter()
                    .find_map(|l| l.strip_prefix("category:"))
                    .unwrap_or("")
                    .to_string();
                let _ = self
                    .storage
                    .archive_upsert_node(t.id as i64, parent_id, "doc", &category);
            }
        }

        // 3. 收编被引用但不在表中的父节点（修复 project 占位节点因内容短未收编的问题）
        //    扫描表中已有的节点，收集它们的 parent_id；如果 parent 不在表中但在记忆系统中，强制收编
        let existing = self.storage.archive_list_nodes();
        let referenced_parents: HashSet<i64> =
            existing.iter().filter_map(|r| r.parent_id).collect();
        for pid in &referenced_parents {
            if self.storage.archive_node_exists(*pid) {
                continue;
            }
            if let Some(t) = meta_by_id.get(&(*pid as u64)) {
                // 推断 type：有子节点 → project，否则 doc
                let has_kids = existing.iter().any(|r| r.parent_id == Some(*pid));
                let node_type = if has_kids { "project" } else { "doc" };
                let category = t
                    .labels
                    .iter()
                    .find_map(|l| l.strip_prefix("category:"))
                    .unwrap_or("")
                    .to_string();
                let _ = self
                    .storage
                    .archive_upsert_node(*pid, None, node_type, &category);
            }
        }

        // 4. 智能分类：把没有 parent 的扁平文档按标签自动归类到 project 下
        //    规则：按主标签匹配到 category project，找不到则保持顶层
        //    只处理 type=doc/code 且 parent_id=NULL 的节点
        let rows2 = self.storage.archive_list_nodes();

        // 一次性重分类：把"系统设计文档"project 下的节点 parent 清空，让二次细分重新归类
        // （独立于 orphan 检查，每次 tree 查询都执行一次，确保细分生效）
        let all_meta = self.space.all_tetras_meta();
        let sys_design_pid: Option<i64> = all_meta
            .iter()
            .find(|m| {
                m.content
                    .lines()
                    .next()
                    .map(|l| l.trim_start_matches("# ").trim())
                    == Some("系统设计文档")
            })
            .map(|m| m.id as i64);
        if let Some(sdp) = sys_design_pid {
            for r in &rows2 {
                if r.parent_id == Some(sdp) {
                    let _ = self.storage.archive_update_node(r.node_id, None, None);
                }
            }
        }

        let rows3 = self.storage.archive_list_nodes();
        let orphans: Vec<&crate::engine::storage::ArchiveNodeRow> = rows3
            .iter()
            .filter(|r| r.parent_id.is_none() && (r.node_type == "doc" || r.node_type == "code"))
            .collect();
        if !orphans.is_empty() {
            // 为每个分类查找或创建 project 节点
            // 分类规则：(标签集合 → project title)
            let categories: &[(&[&str], &str)] = &[
                (&["networking", "protocol"], "协议规范"),
                (&["security"], "安全设计"),
                (&["devops", "infrastructure"], "部署与运维"),
                (&["ai", "ml"], "研究报告"),
                (&["database", "storage"], "数据库设计"),
                (&["biology", "life"], "生命科学"),
                (&["programming", "javascript"], "前端源码"),
                (&["programming", "rust"], "后端源码"),
                (&["system", "architecture"], "系统设计文档"),
                (&["documentation"], "系统设计文档"),
            ];
            // 二次细分：对 system/architecture 类文档按内容关键词进一步分类
            // (关键词 → project title)，优先于上面的粗分类
            let sub_categories: &[(&[&str], &str)] = &[
                (
                    &["审计", "audit", "性能审计", "运行时审计", "交付概览"],
                    "审计报告",
                ),
                (
                    &[
                        "技能",
                        "skill",
                        "记忆存取",
                        "记忆智能",
                        "质量自控",
                        "图谱导航",
                        "知识图谱导航",
                    ],
                    "技能设计",
                ),
                (
                    &["工作记录", "工作全记录", "洞察推送", "闭环完成"],
                    "工作记录",
                ),
                (&["SMRP强化", "EMRP", "响应协议"], "协议强化"),
                (
                    &["开发计划", "UserStory", "claude指南", "用户故事"],
                    "开发规划",
                ),
                (
                    &["调度器", "scheduler", "调度强化", "记忆智能化"],
                    "引擎设计",
                ),
            ];

            // 查找已有的 project 节点（按 title 匹配记忆内容）
            let mut project_cache: HashMap<&str, Option<i64>> = HashMap::new();

            for orphan in &orphans {
                if let Some(t) = meta_by_id.get(&(orphan.node_id as u64)) {
                    // 优先：二次细分（按内容关键词匹配，针对 system/architecture 大类）
                    let matched_sub = sub_categories.iter().find(|(keywords, _)| {
                        let title = t.content.lines().next().unwrap_or("").to_lowercase();
                        let content_lower: String = t
                            .content
                            .chars()
                            .take(200)
                            .collect::<String>()
                            .to_lowercase();
                        keywords.iter().any(|kw| {
                            let kw_l = kw.to_lowercase();
                            title.contains(&kw_l) || content_lower.contains(&kw_l)
                        })
                    });
                    // 兜底：粗分类（按标签）
                    let matched_cat = matched_sub.or_else(|| {
                        categories.iter().find(|(labels, _)| {
                            t.labels.iter().any(|l| labels.contains(&l.as_str()))
                        })
                    });
                    if let Some((_, proj_title)) = matched_cat {
                        // 查找或创建 project
                        let proj_id = if let Some(id) =
                            project_cache.get(proj_title).copied().flatten()
                        {
                            Some(id)
                        } else if let Some(existing) = all_meta.iter().find(|m| {
                            m.content
                                .lines()
                                .next()
                                .map(|l| l.trim_start_matches("# ").trim())
                                == Some(*proj_title)
                        }) {
                            // 确保 project 在表中
                            let _ = self.storage.archive_upsert_node(
                                existing.id as i64,
                                None,
                                "project",
                                "",
                            );
                            project_cache.insert(proj_title, Some(existing.id as i64));
                            Some(existing.id as i64)
                        } else {
                            // 创建新 project 记忆
                            let content = format!("# {}\n\n{}相关文档集合", proj_title, proj_title);
                            let labels = vec!["archive".to_string(), "archive.project".to_string()];
                            if let Ok((pid, _)) = self.api_create_memory(&content, labels) {
                                let _ = self
                                    .storage
                                    .archive_upsert_node(pid as i64, None, "project", "");
                                project_cache.insert(proj_title, Some(pid as i64));
                                Some(pid as i64)
                            } else {
                                None
                            }
                        };
                        // 设置 parent（archive_update_node 更新已有节点的 parent_id）
                        if let Some(pid) = proj_id {
                            let _ =
                                self.storage
                                    .archive_update_node(orphan.node_id, Some(pid), None);
                        }
                    }
                }
            }
        }
    }

    /// 获取档案库完整树结构（从 archive_nodes 表构建，有外键保证完整性）
    pub fn api_archive_tree(&self) -> serde_json::Value {
        // 幂等迁移：把标签式数据同步到表（首次调用后表已填充，后续调用 INSERT OR IGNORE 无操作）
        self.archive_migrate_labels_to_table();

        let meta = self.space.all_tetras_meta();
        let meta_by_id: HashMap<u64, &crate::domain::tetra::TetraMeta> =
            meta.iter().map(|t| (t.id, t)).collect();
        let rows = self.storage.archive_list_nodes();

        // 构建 children map
        let mut children_map: HashMap<i64, Vec<i64>> = HashMap::new();
        for r in &rows {
            if let Some(pid) = r.parent_id {
                children_map.entry(pid).or_default().push(r.node_id);
            }
        }
        let node_map: HashMap<i64, &crate::engine::storage::ArchiveNodeRow> =
            rows.iter().map(|r| (r.node_id, r)).collect();

        // 根节点 = parent_id 为 NULL 或 parent 不在表中的节点
        let roots: Vec<i64> = rows
            .iter()
            .filter_map(|r| {
                match r.parent_id {
                    None => Some(r.node_id),
                    Some(pid) if !node_map.contains_key(&pid) => Some(r.node_id), // parent 已删除 → 提升为根
                    _ => None,
                }
            })
            .collect();

        fn build_tree(
            id: i64,
            meta_map: &HashMap<u64, &crate::domain::tetra::TetraMeta>,
            node_map: &HashMap<i64, &crate::engine::storage::ArchiveNodeRow>,
            children_map: &HashMap<i64, Vec<i64>>,
        ) -> serde_json::Value {
            let row = node_map.get(&id);
            let node = meta_map.get(&(id as u64));
            let content = node.map(|n| n.content.as_str()).unwrap_or("");
            let title = content
                .lines()
                .next()
                .map(|l| l.trim_start_matches("# ").trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| content.chars().take(80).collect());

            let node_type = row.map(|r| r.node_type.as_str()).unwrap_or("doc");
            let category = row.map(|r| r.category.clone()).unwrap_or_default();

            let kids = children_map.get(&id).cloned().unwrap_or_default();
            let children: Vec<serde_json::Value> = kids
                .iter()
                .filter_map(|&cid| {
                    if node_map.contains_key(&cid) {
                        Some(build_tree(cid, meta_map, node_map, children_map))
                    } else {
                        None
                    }
                })
                .collect();

            serde_json::json!({
                "id": id,
                "type": node_type,
                "title": title,
                "category": category,
                "chars": node.map(|n| n.content.len()).unwrap_or(0),
                "timestamp": node.map(|n| n.timestamp).unwrap_or(0),
                "status": "active",
                "children_count": children.len(),
                "children": children,
            })
        }

        let tree: Vec<serde_json::Value> = roots
            .iter()
            .map(|&id| build_tree(id, &meta_by_id, &node_map, &children_map))
            .collect();

        serde_json::json!({ "tree": tree })
    }

    /// 新建档案库节点 = 创建记忆 + 打标签 + 写 archive_nodes 表
    pub fn api_archive_create_node(
        &self,
        parent_id: u64,
        node_type: &str,
        title: &str,
        content: &str,
        category: Option<&str>,
    ) -> Result<u64, String> {
        let mut labels = if node_type == "root" {
            vec!["archive".to_string()]
        } else {
            vec!["archive".to_string(), format!("archive.{}", node_type)]
        };
        // 用 parent:ID 标签直接编码归属关系——不走知识图谱 BelongsTo
        if parent_id > 0 {
            labels.push(format!("parent:{}", parent_id));
        }
        if let Some(cat) = category {
            if !cat.is_empty() {
                labels.push(format!("category:{}", cat));
            }
        }
        let full_content = if content.is_empty() {
            title.to_string()
        } else {
            format!("# {}\n\n{}", title, content)
        };
        let (id, is_new) = self.api_create_memory(&full_content, labels)?;
        // 去重命中时：确保旧节点有正确的 archive + parent 标签
        if !is_new {
            if let Some(mut tetra) = self.space.get_tetrahedron(id) {
                let base_tetra = tetra.data.clone();
                let old_labels = tetra.data.labels.clone();
                let mut changed = false;
                // 补 archive 标签
                if !tetra.data.labels.iter().any(|l| l == "archive") {
                    tetra.data.labels.push("archive".to_string());
                    changed = true;
                }
                if !tetra
                    .data
                    .labels
                    .iter()
                    .any(|l| l == &format!("archive.{}", node_type))
                {
                    tetra.data.labels.push(format!("archive.{}", node_type));
                    changed = true;
                }
                // 补/更新 parent 标签
                let parent_tag = format!("parent:{}", parent_id);
                tetra.data.labels.retain(|l| !l.starts_with("parent:"));
                if parent_id > 0 {
                    tetra.data.labels.push(parent_tag);
                    changed = true;
                }
                // 补 category 标签
                if let Some(cat) = category {
                    if !cat.is_empty() {
                        tetra.data.labels.retain(|l| !l.starts_with("category:"));
                        tetra.data.labels.push(format!("category:{}", cat));
                        changed = true;
                    }
                }
                if changed {
                    let new_labels = tetra.data.labels.clone();
                    if let Err(e) = self.space.patch_payload(id, &base_tetra, tetra.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    // 管道完整性：同步标签索引 + 立即持久化
                    self.gateway
                        .update_label_index(id, &old_labels, &new_labels);
                    self.persist_tetra(id);
                }
            }
        }
        // 写 archive_nodes 表（结构化树，INSERT OR IGNORE 幂等）
        let pid = if parent_id > 0 {
            Some(parent_id as i64)
        } else {
            None
        };
        let _ = self
            .storage
            .archive_upsert_node(id as i64, pid, node_type, category.unwrap_or(""));
        Ok(id)
    }

    /// 删除节点 = 软删除（archive_nodes archived=1）+ 记忆加 archived 标签
    /// 子节点的 parent 自然失效（查询时 archived=0 过滤），不需要手动重定向
    pub fn api_archive_delete_node(&self, node_id: u64) -> Result<(), String> {
        // 软删除：archive_nodes 表 archived=1（数据不丢，可恢复）
        self.storage.archive_soft_delete(node_id as i64)?;

        // 记忆也加 archived 标签（向后兼容标签式查询）
        if let Some(mut tetra) = self.space.get_tetrahedron(node_id) {
            let base_tetra = tetra.data.clone();
            if !tetra.data.labels.iter().any(|l| l == "archived") {
                let old_labels = tetra.data.labels.clone();
                tetra.data.labels.push("archived".to_string());
                let new_labels = tetra.data.labels.clone();
                if let Err(e) = self.space.patch_payload(node_id, &base_tetra, tetra.data) {
                    tracing::warn!("[H2] update_payload failed: {}", e);
                }
                self.gateway
                    .update_label_index(node_id, &old_labels, &new_labels);
                self.persist_tetra(node_id);
            }
        }
        Ok(())
    }

    /// 合并节点 = 创建新记忆 + 原节点建 MergedInto 边 + 原节点加 merged 标签
    pub fn api_archive_merge(
        &self,
        source_ids: &[u64],
        title: &str,
        category: Option<&str>,
    ) -> Result<u64, String> {
        if source_ids.len() < 2 {
            return Err("need at least 2 nodes to merge".into());
        }

        // 拼接内容
        let mut combined = format!("# {}\n\n", title);
        for &sid in source_ids {
            if let Some(t) = self.space.get_tetrahedron(sid) {
                combined.push_str(&format!("---\n{}\n\n", t.data.content));
            }
        }

        // 通过 parent: 标签找到父节点
        let parent_id = self.space.get_tetrahedron(source_ids[0]).and_then(|t| {
            t.data.labels.iter().find_map(|l| {
                l.strip_prefix("parent:")
                    .and_then(|s| s.parse::<u64>().ok())
            })
        });

        let mut labels = vec!["archive".to_string(), "archive.doc".to_string()];
        if let Some(cat) = category {
            labels.push(format!("category:{}", cat));
        }
        // 关键修复：补全 parent: 标签，否则合并产物在树中不可见
        if let Some(pid) = parent_id {
            labels.push(format!("parent:{}", pid));
        }
        let (new_id, _) = self.api_create_memory(&combined, labels)?;

        // 写 archive_nodes 表：合并产物作为新节点
        let pid = parent_id.map(|p| p as i64);
        let _ = self
            .storage
            .archive_upsert_node(new_id as i64, pid, "doc", category.unwrap_or(""));

        // 对每个源建 MergedInto 边 + 加 merged 标签 + 软删除源节点（表）
        for &sid in source_ids {
            self.knowledge.add_relation(
                sid,
                new_id,
                crate::engine::knowledge::RelationType::MergedInto,
                1.0,
            );
            if let Some(mut t) = self.space.get_tetrahedron(sid) {
                let base_t = t.data.clone();
                if !t.data.labels.iter().any(|l| l == "merged") {
                    let old_labels = t.data.labels.clone();
                    t.data.labels.push("merged".to_string());
                    let new_labels = t.data.labels.clone();
                    if let Err(e) = self.space.patch_payload(sid, &base_t, t.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(sid, &old_labels, &new_labels);
                    self.persist_tetra(sid);
                }
            }
            // 软删除源节点（archive_nodes archived=1）
            let _ = self.storage.archive_soft_delete(sid as i64);
            // 管道完整性：源节点的子节点迁移到合并产物下
            let source_children: Vec<u64> = self
                .space
                .all_tetrahedrons()
                .iter()
                .filter(|ct| {
                    ct.data.labels.iter().any(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<u64>().ok())
                            == Some(sid)
                    })
                })
                .map(|ct| ct.id)
                .collect();
            for scid in &source_children {
                if let Some(mut sc) = self.space.get_tetrahedron(*scid) {
                    let base_sc = sc.data.clone();
                    let old_labels = sc.data.labels.clone();
                    sc.data.labels.retain(|l| {
                        l.strip_prefix("parent:")
                            .and_then(|s| s.parse::<u64>().ok())
                            != Some(sid)
                    });
                    sc.data.labels.push(format!("parent:{}", new_id));
                    let new_labels = sc.data.labels.clone();
                    if let Err(e) = self.space.patch_payload(*scid, &base_sc, sc.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(*scid, &old_labels, &new_labels);
                    self.persist_tetra(*scid);
                }
            }
        }

        Ok(new_id)
    }

    /// 移动节点 = 更新 archive_nodes 表的 parent_id（原子操作）+ 同步标签
    pub fn api_archive_move(&self, node_id: u64, new_parent_id: u64) -> Result<(), String> {
        // 表更新（一条 SQL，原子）
        let pid = if new_parent_id > 0 {
            Some(new_parent_id as i64)
        } else {
            None
        };
        self.storage
            .archive_update_node(node_id as i64, pid, None)?;

        // 同步标签（向后兼容）
        if let Some(mut t) = self.space.get_tetrahedron(node_id) {
            let base_t = t.data.clone();
            let old_labels = t.data.labels.clone();
            t.data.labels.retain(|l| !l.starts_with("parent:"));
            if new_parent_id > 0 {
                t.data.labels.push(format!("parent:{}", new_parent_id));
            }
            let new_labels = t.data.labels.clone();
            if let Err(e) = self.space.patch_payload(node_id, &base_t, t.data) {
                tracing::warn!("[H2] update_payload failed: {}", e);
            }
            self.gateway
                .update_label_index(node_id, &old_labels, &new_labels);
            self.persist_tetra(node_id);
        }
        Ok(())
    }

    /// 编辑节点 = 修改记忆内容
    pub fn api_archive_edit_node(
        &self,
        node_id: u64,
        title: Option<&str>,
        content: Option<&str>,
        category: Option<&str>,
    ) -> Result<(), String> {
        let tetra = self
            .space
            .get_tetrahedron(node_id)
            .ok_or("node not found")?;

        // 构建新内容
        let new_content_str = if let Some(t) = title {
            if let Some(c) = content {
                format!("# {}\n\n{}", t, c)
            } else {
                // A title-only patch must preserve everything after the old heading,
                // including blank lines and the body's original line endings.
                match tetra.data.content.split_once('\n') {
                    Some((_, body)) => format!("# {}\n{}", t, body),
                    None => format!("# {}", t),
                }
            }
        } else if let Some(c) = content {
            // 保持原标题，只替换内容部分
            let old_first_line = tetra.data.content.lines().next().unwrap_or("");
            format!("{}\n\n{}", old_first_line, c)
        } else {
            // 只改 category，内容不变
            tetra.data.content.clone()
        };

        // 内容变更时走 gateway.update_content（重建 embedding + content_hash + HNSW）
        if new_content_str != tetra.data.content {
            self.gateway.update_content(node_id, &new_content_str)?;
        }

        // category 标签变更 + 同步到 archive_nodes 表
        if let Some(cat) = category {
            if let Some(mut t) = self.space.get_tetrahedron(node_id) {
                let base_t = t.data.clone();
                let old_labels = t.data.labels.clone();
                t.data.labels.retain(|l| !l.starts_with("category:"));
                if !cat.is_empty() {
                    t.data.labels.push(format!("category:{}", cat));
                }
                let new_labels = t.data.labels.clone();
                if old_labels != new_labels {
                    if let Err(e) = self.space.patch_payload(node_id, &base_t, t.data) {
                        tracing::warn!("[H2] update_payload failed: {}", e);
                    }
                    self.gateway
                        .update_label_index(node_id, &old_labels, &new_labels);
                    self.persist_tetra(node_id);
                }
            }
            // 同步到表
            let _ = self
                .storage
                .archive_update_node(node_id as i64, None, Some(cat));
        }

        Ok(())
    }

    /// 确保档案库根节点存在，返回其 ID
    pub fn api_archive_ensure_root(&self) -> u64 {
        // 查找已有根节点
        let meta = self.space.all_tetras_meta();
        if let Some(root) = meta.iter().find(|t| {
            t.labels.iter().any(|l| l == "archive")
                && !t.labels.iter().any(|l| l.starts_with("archive."))
        }) {
            return root.id;
        }
        // 创建根节点
        let (id, _) = self
            .api_create_memory("Epicode Archive Root", vec!["archive".to_string()])
            .unwrap_or((0, false));
        id
    }

    pub fn api_decay_relations(&self) -> usize {
        self.gateway.decay_relations()
    }

    /// P4 治理 API 共用：为指定记忆原子地追加一组标签。
    /// 同时更新 Space、gateway label_index，并标记 dirty 让 janitor 持久化。
    /// 返回 (是否变更, 旧标签, 新标签)。
    pub fn api_add_labels(
        &self,
        id: TetraId,
        labels_to_add: &[&str],
    ) -> Result<(bool, Vec<String>, Vec<String>), String> {
        let tetra = self
            .space
            .get_tetrahedron(id)
            .ok_or_else(|| format!("memory {} not found", id))?;
        let old_labels = tetra.data.labels.clone();
        let mut new_labels = old_labels.clone();
        let mut changed = false;
        for lbl in labels_to_add {
            if lbl.is_empty() {
                continue;
            }
            if !new_labels.iter().any(|l| l == lbl) {
                new_labels.push(lbl.to_string());
                changed = true;
            }
        }
        if !changed {
            return Ok((false, old_labels.clone(), old_labels));
        }
        // op-log: 在写锁内对当前标签做幂等追加,而不是写回锁外算好的整份列表
        // (旧实现会覆盖锁外窗口内并发写入的标签)
        let ops: Vec<crate::domain::ops::MemoryOp> = labels_to_add
            .iter()
            .filter(|l| !l.is_empty())
            .map(|l| crate::domain::ops::MemoryOp::AddLabel(l.to_string()))
            .collect();
        self.space
            .apply_ops(id, &ops)
            .map_err(|e| format!("update labels {} failed: {}", id, e))?;
        let new_labels = self
            .space
            .get_tetrahedron(id)
            .map(|t| t.data.labels)
            .unwrap_or(new_labels);
        // 维护 gateway label_index（让后续 list_by_labels / list_projects 可见）
        self.gateway
            .update_label_index(id, &old_labels, &new_labels);
        // 持久化（异步 dirty 标记）
        self.persist_tetra(id);
        tracing::info!("[P4] api_add_labels #{}: +{:?}", id, labels_to_add);
        Ok((true, old_labels, new_labels))
    }
}
