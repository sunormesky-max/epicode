use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use parking_lot::{Mutex, RwLock};

use crate::domain::space::Space;
use crate::domain::tetra::{MemoryPayload, TetraId};
use crate::engine::vector::VectorLayer;

fn strip_session_prefix(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('[') {
        return text;
    }
    if let Some(end) = trimmed.find(']') {
        let bracket = &trimmed[..end + 1];
        if bracket.starts_with("[session_")
            || bracket.starts_with("[session|")
            || bracket.starts_with("[finding]")
            || bracket.starts_with("[decision]")
            || bracket.starts_with("[pattern]")
            || bracket.starts_with("[preference]")
        {
            return trimmed[end + 1..].trim_start();
        }
        if bracket.contains("|") && (bracket.contains("am") || bracket.contains("pm")) && bracket.len() < 80 {
            let after = trimmed[end + 1..].trim_start();
            if !after.is_empty() {
                return after;
            }
        }
    }
    text
}

use super::cognitive::CognitiveEngine;
use super::embedding::EmbeddingService;
use super::hnsw::HnswIndex;
use super::knowledge::KnowledgeGraph;

pub struct SearchEngineState {
    pub hnsw: RwLock<HnswIndex>,
    pub search_total: AtomicU64,
    pub search_hits: AtomicU64,
    pub search_miss_queries: Mutex<std::collections::VecDeque<String>>,
    pub search_top_labels: Mutex<HashMap<String, u32>>,
    pub access_counts: Mutex<HashMap<TetraId, u32>>,
    df_cache: Mutex<Option<DfCache>>,
    /// 文档 token 缓存：tetra_id → (tokens, doc_len)，避免 BM25 评分时重复 tokenize
    doc_token_cache: Mutex<HashMap<TetraId, (Vec<String>, usize)>>,
}

struct DfCache {
    df_map: HashMap<String, usize>,
    doc_count: usize,
    avg_dl: f64,
    tetra_count: usize,
}

impl SearchEngineState {
    pub fn new(hnsw: HnswIndex) -> Self {
        Self {
            hnsw: RwLock::new(hnsw),
            search_total: AtomicU64::new(0),
            search_hits: AtomicU64::new(0),
            search_miss_queries: Mutex::new(std::collections::VecDeque::new()),
            search_top_labels: Mutex::new(HashMap::new()),
            access_counts: Mutex::new(HashMap::new()),
            df_cache: Mutex::new(None),
            doc_token_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn access_counts_snapshot(&self) -> Vec<(TetraId, u32)> {
        self.access_counts.lock().iter().map(|(k, v)| (*k, *v)).collect()
    }

    pub fn invalidate_df_cache(&self) {
        *self.df_cache.lock() = None;
        self.doc_token_cache.lock().clear();
    }

    /// 获取或构建文档 token（缓存），避免 BM25 评分时对同一文档重复 tokenize
    fn get_doc_tokens(&self, tetra_id: TetraId, content: &str, aliases: &[String]) -> Vec<String> {
        {
            let cache = self.doc_token_cache.lock();
            if let Some((tokens, _)) = cache.get(&tetra_id) {
                return tokens.clone();
            }
        }
        let content_lower = strip_session_prefix(content).to_lowercase();
        let alias_text = aliases.join(" ").to_lowercase();
        let doc_text = format!("{} {}", content_lower, alias_text);
        let tokens = tokenize(&doc_text);
        let mut cache = self.doc_token_cache.lock();
        cache.insert(tetra_id, (tokens.clone(), tokens.len()));
        tokens
    }

    fn get_or_build_df(&self, tetras: &[crate::domain::tetra::Tetrahedron]) -> (HashMap<String, usize>, usize, f64) {
        // P4修复:复用 doc_token_cache 避免 build_df_map 重复 tokenize 全量文档。
        // 先检查 df_cache(快路径),miss 时用 get_doc_tokens(命中 token 缓存)构建。
        {
            let cache = self.df_cache.lock();
            if let Some(ref c) = *cache {
                if c.tetra_count == tetras.len() {
                    return (c.df_map.clone(), c.doc_count, c.avg_dl);
                }
            }
        }
        // 用 doc_token_cache 复用已 tokenize 的结果(避免重新 tokenize 全量)
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut total_dl: f64 = 0.0;
        let doc_count = tetras.len();
        for t in tetras {
            let doc_tokens = self.get_doc_tokens(t.id, &t.data.content, &t.data.aliases);
            total_dl += doc_tokens.len() as f64;
            let mut seen = HashSet::new();
            for term in doc_tokens {
                if seen.insert(term.clone()) {
                    *df.entry(term).or_insert(0) += 1;
                }
            }
        }
        let avg_dl = if doc_count > 0 { total_dl / doc_count as f64 } else { 0.0 };
        let mut cache = self.df_cache.lock();
        *cache = Some(DfCache {
            df_map: df.clone(),
            doc_count,
            avg_dl,
            tetra_count: doc_count,
        });
        (df, doc_count, avg_dl)
    }
}

pub struct SearchCtx<'a> {
    pub state: &'a SearchEngineState,
    pub space: &'a Space,
    pub knowledge: &'a KnowledgeGraph,
    pub cognitive: &'a CognitiveEngine,
    pub embedding: &'a EmbeddingService,
    pub label_index: &'a Mutex<HashMap<String, Vec<TetraId>>>,
}

/// Phase 1 检索可信度重建:搜索模式控制评分通道
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// 默认: 向量 + BM25 混合(现有行为, 向后兼容)
    Hybrid,
    /// 纯 BM25×10, alias 置顶, 不向量回填, 短路语义展开与 rerank
    /// 用于精确 token 检索: 搜索结构化标识符/自身内容/已知短语
    Exact,
    /// 纯向量(关 BM25), 适合概念相似性查询
    Semantic,
    /// 语义召回 + 知识图谱关联扩展(Phase 1 先走 Hybrid 召回, KG expand 后续完善)
    Graph,
    /// D2.2 模式路由器: 按查询形态自动选 semantic 或 graph+PPR
    Auto,
    /// D2.2b RRF融合: semantic+graph双跑, 排序倒数融合(Reciprocal Rank Fusion, k=60)
    /// 规则路由失败(55.0%)后的正确答案——不预测类型, 直接取两模式之长
    Fusion,
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

impl SearchMode {
    /// 从字符串解析(用于 MCP / REST 参数)
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "exact" => SearchMode::Exact,
            "semantic" => SearchMode::Semantic,
            "graph" => SearchMode::Graph,
            "auto" => SearchMode::Auto,
            "fusion" => SearchMode::Fusion,
            _ => SearchMode::Hybrid,
        }
    }
}

/// D2.2: 查询是否需要图扩散(时序推理/跨会话聚合特征)
/// 启发式线索表 — 由 LongMemEval 分类别成绩背书
fn query_needs_diffusion(query: &str) -> bool {
    const TEMPORAL_CUES: &[&str] = &[
        "first", "before", "after", "when did", "when was", "how many days", "how long",
        "used to", "previously", "no longer", "switched", "changed", "moved to",
        "最初", "之前", "之后", "先", "几月", "哪年", "什么时候", "多久", "换了", "改了",
    ];
    const AGGREGATION_CUES: &[&str] = &[
        "total", "both", "combined", "all the", "how many", "in total", "altogether",
        "一共", "总共", "都", "哪些",
    ];
    let q = query.to_lowercase();
    TEMPORAL_CUES.iter().any(|c| q.contains(c)) || AGGREGATION_CUES.iter().any(|c| q.contains(c))
}

/// 每条搜索结果的命中来源(可解释性, Phase 1)
/// 记录这条结果是通过哪些信号命中的, 便于调用方判断可信度
pub type MatchedBy = Vec<&'static str>;

#[derive(Debug, Default)]
pub struct SearchFilters {
    pub labels: Option<Vec<String>>,
    pub min_importance: Option<f64>,
    pub max_importance: Option<f64>,
    pub since_ts: Option<i64>,
    pub until_ts: Option<i64>,
    pub project: Option<String>,
    /// Phase 1: 搜索模式(默认 Hybrid 向后兼容)
    pub mode: SearchMode,
    /// Phase 1: 严格过滤模式 — 只返回精确匹配过滤条件的结果, 不做语义回填
    pub strict_filter: bool,
    /// D4: 时点查询 — 只返回 as_of 时刻有效的记忆
    /// (valid_from <= as_of 且 (valid_to为空 或 valid_to > as_of))
    pub as_of: Option<i64>,
}

fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x3000..=0x303F).contains(&cp)
        || (0x3040..=0x309F).contains(&cp)
        || (0x30A0..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
}

fn is_real_english_word(word: &str) -> bool {
    const COMMON: &[&str] = &[
        "the","be","to","of","and","a","in","that","have","i","it","for","not","on","with","he",
        "as","you","do","at","this","but","his","by","from","they","we","say","her","she","or","an",
        "will","my","one","all","would","there","their","what","so","up","out","if","about","who",
        "get","which","go","me","when","make","can","like","time","no","just","him","know","take",
        "people","into","year","your","good","some","could","them","see","other","than","then","now",
        "look","only","come","its","over","think","also","back","after","use","two","how","our","work",
        "first","well","way","even","new","want","because","any","these","give","day","most","us",
        "search","query","memory","find","data","list","create","update","delete","pattern","skill",
        "recall","context","session","summary","decision","feedback","identity","knowledge","reasoning",
        "error","help","test","rust","code","build","run","start","stop","config","log","debug",
        "info","warn","trace","system","admin","user","name","type","value","key","token","api",
    ];
    let lower = word.to_lowercase();
    COMMON.contains(&lower.as_str())
}

fn looks_like_keyboard_smash(word: &str) -> bool {
    if word.len() < 5 { return false; }
    let lower = word.to_lowercase();
    if lower.contains("asdf") || lower.contains("qwer") || lower.contains("zxcv") { return true; }
    if lower.contains("1234") || lower.contains("aaaa") || lower.contains("qqqq") { return true; }
    let bytes = lower.as_bytes();
    let mut consecutive_consonants = 0;
    let mut max_consecutive = 0;
    for &b in bytes {
        if b.is_ascii_alphabetic() && !"aeiou".contains(b as char) {
            consecutive_consonants += 1;
            max_consecutive = max_consecutive.max(consecutive_consonants);
        } else {
            consecutive_consonants = 0;
        }
    }
    if max_consecutive >= 5 { return true; }
    false
}

pub fn is_low_quality_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() { return true; }
    let chars: Vec<char> = trimmed.chars().collect();
    let has_cjk = chars.iter().any(|c| is_cjk(*c));
    if has_cjk { return false; }
    let alpha_count = chars.iter().filter(|c| c.is_alphabetic()).count() as f64;
    let total = chars.len() as f64;
    if total < 3.0 { return true; }
    if alpha_count / total < 0.5 { return true; }
    let tokens = tokenize(trimmed);
    if tokens.is_empty() { return true; }
    if tokens.iter().any(|t| is_real_english_word(t)) { return false; }
    if tokens.len() == 1 {
        let t = &tokens[0];
        if looks_like_keyboard_smash(t) { return true; }
        let unique_chars: std::collections::HashSet<char> = t.chars().collect();
        if t.len() >= 3 && unique_chars.len() <= 2 { return true; }
    }
    false
}

pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut word_buf = String::new();
    let mut cjk_buf = String::new();

    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !word_buf.is_empty() { tokens.push(word_buf.clone()); word_buf.clear(); }
            if !cjk_buf.is_empty() { flush_cjk(&cjk_buf, &mut tokens); cjk_buf.clear(); }
        } else if is_cjk(ch) {
            if !word_buf.is_empty() { tokens.push(word_buf.clone()); word_buf.clear(); }
            cjk_buf.push(ch);
        } else if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '/' || ch == ':' {
            if !cjk_buf.is_empty() { flush_cjk(&cjk_buf, &mut tokens); cjk_buf.clear(); }
            word_buf.push(ch);
        } else {
            if !word_buf.is_empty() { tokens.push(word_buf.clone()); word_buf.clear(); }
            if !cjk_buf.is_empty() { flush_cjk(&cjk_buf, &mut tokens); cjk_buf.clear(); }
        }
    }
    if !word_buf.is_empty() { tokens.push(word_buf); }
    if !cjk_buf.is_empty() { flush_cjk(&cjk_buf, &mut tokens); }

    tokens
}

fn flush_cjk(buf: &str, tokens: &mut Vec<String>) {
    let chars: Vec<char> = buf.chars().collect();
    if chars.is_empty() { return; }
    if chars.len() == 1 {
        tokens.push(chars[0].to_string());
        return;
    }
    for w in chars.windows(2) {
        tokens.push(format!("{}{}", w[0], w[1]));
    }
    if chars.len() <= 3 {
        tokens.push(buf.to_string());
    }
}

/// Phase 1+: camelCase 子 token 扩展(exact 模式 query 端, 不破坏 df_cache)
///
/// 问题: tokenize 把 "MarkdownText.tsx" 当成一个 token "markdowntext.tsx"。
/// 如果记忆里存的是 "MarkdownText.tsx"(小写后一致)能命中, 但如果记忆里是
/// "MarkdownText"(无 .tsx)或分开写的就匹配不上。
///
/// 解法: 对 camelCase token(含大小写边界)额外拆出子 token。
/// 例: "DashboardCognitive" → 原始 + "dashboard" + "cognitive"
/// 原始 token 保留(完整标识符命中), 子 token 补充(部分匹配命中)。
///
/// 关键: 只在 query 端扩展, 不改 tokenize(否则 df_cache/doc_token_cache 全部失效)。
fn expand_camel_query_tokens(tokens: &[String]) -> Vec<String> {
    let mut expanded = Vec::with_capacity(tokens.len() * 2);
    for tok in tokens {
        expanded.push(tok.clone()); // 保留原始 token
        let lower = tok.to_lowercase();
        // 检测 camelCase 边界: 小写→大写 的转换点
        let chars: Vec<char> = tok.chars().collect();
        if chars.len() < 4 {
            continue; // 太短不值得拆(如 "HNSW" 是缩写不是 camelCase)
        }
        let mut start = 0;
        let mut has_split = false;
        for i in 1..chars.len() {
            // 小写→大写 边界(如 "markdown|Text")
            if chars[i - 1].is_lowercase() && chars[i].is_uppercase() {
                let sub: String = chars[start..i].iter().collect::<String>().to_lowercase();
                if sub.len() >= 2 && sub != lower {
                    expanded.push(sub);
                    has_split = true;
                }
                start = i;
            }
        }
        // 尾部子 token
        if has_split && start < chars.len() {
            let sub: String = chars[start..].iter().collect::<String>().to_lowercase();
            if sub.len() >= 2 {
                expanded.push(sub);
            }
        }
        // 处理 .tsx/.rs/.js 后缀: 把 "markdowntext.tsx" 拆成 "markdowntext" + "tsx"
        // 这样即使记忆里存的是 "MarkdownText"(无后缀)也能命中
        if let Some(dot_pos) = lower.find('.') {
            let base = &lower[..dot_pos];
            let ext_raw = &lower[dot_pos + 1..];
            // ext 去掉尾部标点(记忆里 "MarkdownText.tsx:" → tokenize 生成 "markdowntext.tsx:",
            // 文档侧 token 带冒号, query 侧 "tsx" 不带; 反之亦然。生成纯净 ext)
            let ext: String = ext_raw.chars().take_while(|c| c.is_alphanumeric()).collect();
            if base.len() >= 3 && !expanded.contains(&base.to_string()) {
                expanded.push(base.to_string());
            }
            if ext.len() >= 2 && !expanded.contains(&ext) {
                expanded.push(ext);
            }
        }
        // 通用尾部清理: 去掉尾部非字母数字字符(: ; , 等)
        // 记忆里 "file.rs:" → token "file.rs:", query "file.rs" 不匹配
        let trimmed: String = lower.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-').to_string();
        if trimmed.len() >= 3 && trimmed != lower && !expanded.contains(&trimmed) {
            expanded.push(trimmed);
        }
    }
    expanded
}

/// Phase 1: 搜索结果带命中来源(5-tuple), exact 模式专用返回类型
pub type ScoredWithMatch = (TetraId, f64, f64, MemoryPayload, MatchedBy);

/// Phase 1 检索可信度重建: 带模式的搜索入口
///
/// 路由:
/// - Exact → search_exact() 纯 BM25, 不向量, 返回 matched_by
/// - Hybrid/Semantic/Graph → 老 search() 逻辑, matched_by 为空 vec
///
/// gateway 层调用此函数; 老 search() 保留向后兼容(内部调本函数丢 matched_by)
pub fn search_with_mode(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    vector: Option<&crate::engine::vector::VectorLayer>,
    filters: Option<&SearchFilters>,
    mode: SearchMode,
) -> Result<Vec<ScoredWithMatch>, String> {
    match mode {
        SearchMode::Exact => {
            // exact 模式: 不做 cognitive translate_and_expand(会语义展开), 用原始 query
            let scored = search_exact(ctx, query, k, filters)?;
            Ok(scored)
        }
        SearchMode::Semantic => search_semantic(ctx, query, k, vector, filters),
        SearchMode::Graph => search_graph(ctx, query, k, vector, filters),
        // D2.2 模式路由器: 路由表由 LongMemEval 全量500题实测定义——
        // 时序/聚合类查询 → graph+PPR(temporal 57.9/multi-session 57.9 优于 semantic 49.6/47.4),
        // 其余(知识/事实类) → semantic(91.0/89.3/85.7 占优, 且 p50 52ms vs 254ms)
        // D2.2b: RRF融合 — 双模式并行, score(d)=Σ 1/(60+rank_i), 无路由智能取两模式之长
        SearchMode::Fusion => {
            let sem = search_semantic(ctx, query, k, vector, filters)?;
            let grp = search_graph(ctx, query, k, vector, filters)?;
            let mut rrf: std::collections::HashMap<u64, (f64, usize, usize)> = std::collections::HashMap::new();
            let mut payloads: std::collections::HashMap<u64, (f64, crate::domain::tetra::MemoryPayload, Vec<&'static str>)> = std::collections::HashMap::new();
            for (rank, (id, _sim, mass, payload, mut src)) in sem.into_iter().enumerate() {
                let e = rrf.entry(id).or_insert((0.0, rank, usize::MAX));
                e.0 += 1.0 / (60.0 + rank as f64);
                if rank < e.1 { e.1 = rank; }
                payloads.insert(id, (mass, payload, src.clone()));
                if let Some(p) = payloads.get_mut(&id) { p.2 = src.clone(); }
            }
            for (rank, (id, _sim, mass, payload, src)) in grp.into_iter().enumerate() {
                let e = rrf.entry(id).or_insert((0.0, usize::MAX, rank));
                e.0 += 1.0 / (60.0 + rank as f64);
                if rank < e.2 { e.2 = rank; }
                payloads.entry(id).or_insert((mass, payload, src.clone()));
                if let Some(p) = payloads.get_mut(&id) { p.2.extend(src); }
            }
            let mut fused: Vec<(u64, f64, (f64, crate::domain::tetra::MemoryPayload, Vec<&'static str>))> =
                rrf.into_iter().map(|(id, (score, _, _))| (id, score, payloads.remove(&id).unwrap())).collect();
            fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            Ok(fused.into_iter().take(k).map(|(id, score, (mass, payload, src))| (id, score, mass, payload, src)).collect())
        }
        SearchMode::Auto => {
            if query_needs_diffusion(query) {
                tracing::debug!("[Search] auto-routed: graph+PPR (temporal/aggregation cues)");
                search_graph(ctx, query, k, vector, filters)
            } else {
                tracing::debug!("[Search] auto-routed: semantic (fact/knowledge query)");
                search_semantic(ctx, query, k, vector, filters)
            }
        }
        SearchMode::Hybrid => {
            let results = search(ctx, query, k, vector, filters)?;
            Ok(results.into_iter().map(|(id, s, m, p)| (id, s, m, p, Vec::new())).collect())
        }
    }
}

fn search_semantic(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    vector: Option<&crate::engine::vector::VectorLayer>,
    filters: Option<&SearchFilters>,
) -> Result<Vec<ScoredWithMatch>, String> {
    let qe = match compute_query_embedding(query, vector, ctx.embedding).filter(|e| !e.is_empty()) {
        Some(e) => e,
        None => {
            tracing::info!("[Search] semantic mode has no query embedding, returning empty");
            return Ok(vec![]);
        }
    };
    let hnsw = ctx.state.hnsw.read();
    let hits = hnsw.search_knn(&qe, (k * 3).max(30), (k * 5).max(80));
    drop(hnsw);
    let mut out = Vec::new();
    for (id, sim) in hits {
        if let Some(t) = ctx.space.get_tetrahedron(id) {
            if !passes_filters(&t, filters) { continue; }
            out.push((id, sim, t.mass, t.data, vec!["vector"]));
        }
    }
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(k);
    Ok(out)
}

fn search_graph(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    vector: Option<&crate::engine::vector::VectorLayer>,
    filters: Option<&SearchFilters>,
) -> Result<Vec<ScoredWithMatch>, String> {
    let seeds = search(ctx, query, k.max(16), vector, filters)?;

    // ── D2.1: Personalized PageRank 海马式扩散激活(HippoRAG式) ──
    // 曾为单跳邻居扩展: gsim=seed_sim×strength, 深层关联记忆永远够不到。
    // PPR: rank = 0.15·seed + 0.85·M·rank, 受限BFS建邻接(3跳/2000节点封顶), 15次幂迭代
    // 融合: final = 0.6·ppr_norm + 0.4·seed_norm (检索精度与扩散召回的平衡)
    const PPR_RESTART: f64 = 0.15;
    const PPR_ITER: usize = 15;
    const PPR_FRONTIER: usize = 2000;

    let seed_vec: Vec<(u64, f64)> = seeds.iter().take(16).map(|r| (r.0, r.1)).collect();
    let seed_sum: f64 = seed_vec.iter().map(|r| r.1).sum();
    if seed_sum <= 0.0 || ctx.knowledge.relation_count() == 0 {
        // 无KG或零分种子: 退化为纯base检索
        return Ok(seeds.into_iter().take(k).map(|(id, sim, mass, payload)| (id, sim, mass, payload, vec!["hybrid"])).collect());
    }
    let mut seed_dist: HashMap<u64, f64> = HashMap::new();
    for (id, s) in &seed_vec { seed_dist.insert(*id, s / seed_sum); }

    // 1) 受限BFS建邻接(含过滤: 无效记忆不入图)
    let mut adj: HashMap<u64, Vec<(u64, f64)>> = HashMap::new();
    let mut frontier: Vec<u64> = seed_dist.keys().copied().collect();
    let mut node_count = frontier.len();
    for _hop in 0..3 {
        let mut next_frontier: Vec<u64> = Vec::new();
        for u in &frontier {
            if adj.contains_key(u) { continue; }
            let edges: Vec<(u64, f64)> = ctx.knowledge.query_relations(*u).into_iter()
                .filter(|(v, _, _)| passes_filters_id(ctx, *v, filters))
                .map(|(v, _, w)| (v, w.max(0.05)))
                .collect();
            for (v, _) in &edges {
                if !adj.contains_key(v) && node_count < PPR_FRONTIER {
                    next_frontier.push(*v); node_count += 1;
                }
            }
            adj.insert(*u, edges);
        }
        if next_frontier.is_empty() || node_count >= PPR_FRONTIER { break; }
        frontier = next_frontier;
    }
    // 叶子节点的空邻接(幂迭代需要)
    let leaf_targets: Vec<u64> = {
        let mut v: Vec<u64> = Vec::new();
        for edges in adj.values() { for (t, _) in edges { v.push(*t); } }
        v
    };
    for t in leaf_targets { adj.entry(t).or_default(); }

    // 2) 幂迭代
    let mut rank: HashMap<u64, f64> = seed_dist.clone();
    for _ in 0..PPR_ITER {
        let mut next: HashMap<u64, f64> = HashMap::with_capacity(rank.len());
        for (u, edges) in &adj {
            let ru = *rank.get(u).unwrap_or(&0.0);
            if ru <= 1e-12 || edges.is_empty() { continue; }
            let wsum: f64 = edges.iter().map(|(_, w)| *w).sum();
            if wsum <= 0.0 { continue; }
            for (v, w) in edges {
                *next.entry(*v).or_insert(0.0) += ru * w / wsum;
            }
        }
        for v in next.values_mut() { *v *= 1.0 - PPR_RESTART; }
        for (s, v) in &seed_dist { *next.entry(*s).or_insert(0.0) += PPR_RESTART * v; }
        rank = next;
    }

    // 3) 融合与输出
    let ppr_max = rank.values().cloned().fold(0.0_f64, f64::max).max(1e-9);
    let seed_max = seed_vec.iter().map(|r| r.1).fold(0.0_f64, f64::max).max(1e-9);
    let mut scored: Vec<(u64, f64, bool)> = Vec::new();
    for (id, p) in &rank {
        let s = seed_vec.iter().find(|r| r.0 == *id).map(|r| r.1).unwrap_or(0.0);
        scored.push((*id, 0.6 * (p / ppr_max) + 0.4 * (s / seed_max), s > 0.0));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut out: Vec<ScoredWithMatch> = Vec::new();
    for (id, score, is_seed) in scored.into_iter().take(k) {
        if let Some(t) = ctx.space.get_tetrahedron(id) {
            let src: Vec<&str> = if is_seed { vec!["hybrid", "kg-ppr"] } else { vec!["kg-ppr"] };
            out.push((id, score, t.mass, t.data, src));
        }
    }
    Ok(out)
}

/// PPR辅助: 按id过滤(建图阶段剔除无效记忆, 防扩散污染)
fn passes_filters_id(ctx: &SearchCtx, id: u64, filters: Option<&SearchFilters>) -> bool {
    match ctx.space.get_tetrahedron(id) {
        Some(t) => passes_filters(&t, filters),
        None => false,
    }
}

/// Phase 1 检索可信度重建: Exact 模式专用搜索
///
/// 契约:
/// - 跳过 query_embedding 计算(省 ONNX/HTTP 调用)
/// - 跳过 HNSW 向量召回(完全旁路向量通道)
/// - 走 label_index 倒排索引找候选 + 全量 BM25 fallback
/// - score_tetra_exact 评分(无精确命中归零)
/// - 返回 matched_by 命中来源
fn search_exact(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    filters: Option<&SearchFilters>,
) -> Result<Vec<ScoredWithMatch>, String> {
    // exact 模式不做 gibberish 检测 — 精确标识符(如 "tm-3fc7bdde")可能被误判
    let raw_tokens = tokenize(query);
    if raw_tokens.is_empty() {
        return Ok(vec![]);
    }
    // Phase 1+: camelCase 子 token 扩展, 让 "MarkdownText.tsx" 也能命中存了
    // "MarkdownText" 或 "markdown" 的记忆(query 端扩展, 不破坏 df_cache)
    let query_tokens = expand_camel_query_tokens(&raw_tokens);

    let all_tetras = ctx.space.all_tetrahedrons();
    let (df_map, doc_count, avg_dl) = ctx.state.get_or_build_df(&all_tetras);
    let df_map_ref = &df_map;
    let state_ref = &ctx.state;
    let access_counts_snapshot = ctx.state.access_counts.lock().clone();

    // 候选集: label_index 倒排(精确 token 命中的记忆) + label 子串匹配
    let label_idx = ctx.label_index.lock();
    let mut candidate_ids: HashSet<u64> = HashSet::new();
    for tok in &query_tokens {
        if let Some(ids) = label_idx.get(tok) {
            for &id in ids { candidate_ids.insert(id); }
        }
    }
    // label 本身作为 token 出现在 query 里
    for (label, ids) in label_idx.iter() {
        let label_lower = label.to_lowercase();
        if query_tokens.iter().any(|w| label_lower.contains(w.as_str())) {
            for &id in ids { candidate_ids.insert(id); }
        }
    }
    drop(label_idx);

    // strict_filter 模式: 如果设了 label/project 过滤, 只在这些过滤集里找(不 fallback 全量)
    let strict = filters.map(|f| f.strict_filter).unwrap_or(false);

    let scored: Vec<ScoredWithMatch> = if candidate_ids.is_empty() && !strict {
        // 无 label 候选且非严格模式: 全量 BM25 fallback(exact 模式仍需扫全量找精确命中)
        all_tetras.into_iter()
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                let doc_tokens = state_ref.get_doc_tokens(t.id, &t.data.content, &t.data.aliases);
                score_tetra_exact(&t, &query_tokens, avg_dl, doc_count, df_map_ref, &doc_tokens)
            })
            .collect()
    } else if candidate_ids.is_empty() {
        // strict 模式但无候选: 返回空(不回填)
        vec![]
    } else {
        all_tetras.into_iter()
            .filter(|t| candidate_ids.contains(&t.id))
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                let doc_tokens = state_ref.get_doc_tokens(t.id, &t.data.content, &t.data.aliases);
                score_tetra_exact(&t, &query_tokens, avg_dl, doc_count, df_map_ref, &doc_tokens)
            })
            .collect()
    };

    // 排序: 分数降序
    let mut sorted = scored;
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // 过滤掉 0 分结果(无精确命中的)
    sorted.retain(|(_, score, _, _, _)| *score > 0.0);

    // 统计
    ctx.state.search_total.fetch_add(1, AtomicOrdering::Relaxed);
    if let Some((id, score, _, payload, _)) = sorted.first() {
        if *score > 0.25 {
            ctx.state.search_hits.fetch_add(1, AtomicOrdering::Relaxed);
            {
                let mut top_labels = ctx.state.search_top_labels.lock();
                for label in &payload.labels {
                    *top_labels.entry(label.clone()).or_insert(0) += 1;
                }
                if top_labels.len() > 200 {
                    let mut s: Vec<_> = top_labels.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    s.sort_by(|a, b| b.1.cmp(&a.1));
                    s.truncate(150);
                    top_labels.clear();
                    for (k, v) in s { top_labels.insert(k, v); }
                }
            }
            *ctx.state.access_counts.lock().entry(*id).or_insert(0) += 1;
        }
    }

    sorted.truncate(k);
    Ok(sorted)
}

pub fn search(
    ctx: &SearchCtx,
    query: &str,
    k: usize,
    vector: Option<&crate::engine::vector::VectorLayer>,
    filters: Option<&SearchFilters>,
) -> Result<Vec<(TetraId, f64, f64, MemoryPayload)>, String> {
    let search_query = if ctx.cognitive.enabled() {
        match ctx.cognitive.translate_and_expand(query) {
            Ok((en, _translated)) => en,
            Err(e) => {
                tracing::debug!("[Gateway] translate_and_expand failed: {}, using raw query", e);
                query.to_string()
            }
        }
    } else {
        query.to_string()
    };
    let gibberish_query = is_low_quality_query(&search_query);
    if gibberish_query {
        tracing::warn!("[Search] low quality query detected: {:?}", query);
    }
    let query_embedding = compute_query_embedding(&search_query, vector, ctx.embedding);
    // 检索突破：防御ONNX超时返回空向量（vec![]），空向量进HNSW会导致所有distance=0→随机召回
    let query_embedding = query_embedding.filter(|qe| !qe.is_empty());

    let hnsw_candidates = if let Some(ref qe) = query_embedding {
        let hnsw = ctx.state.hnsw.read();
        let candidate_k = (k * 5).max(50);
        let ef = (k * 5).max(100);
        hnsw.search_knn(qe, candidate_k, ef)
    } else {
        vec![]
    };

    let all_tetras = ctx.space.all_tetrahedrons();
    let (df_map, doc_count, avg_dl) = ctx.state.get_or_build_df(&all_tetras);
    let df_map_ref = &df_map;

    // 预计算 query tokens 一次，所有评分函数复用（避免每候选重复 tokenize 6+ 次）
    let query_tokens = tokenize(&search_query);
    let query_tokens_ref = &query_tokens;
    // M8修复:一次搜索内"当前时间"是常量,取一次传给 score_tetra(避免每候选系统调用)
    let now_ts_search = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;

    let state_ref = &ctx.state;
    // P1修复:评分前 clone access_counts 快照后立即释放锁,
    // 避免全程持锁跨评分+排序+LLM rerank 阻塞并发搜索和 flush。
    let access_counts_snapshot = ctx.state.access_counts.lock().clone();
    let mut scored: Vec<(TetraId, f64, f64, MemoryPayload)> = if !hnsw_candidates.is_empty() {
        // HNSW 路径：只取候选 tetra，避免遍历全部
        let candidate_set: HashSet<u64> = hnsw_candidates.iter().map(|(id, _)| *id).collect();
        all_tetras.into_iter()
            .filter(|t| candidate_set.contains(&t.id))
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                let doc_tokens = state_ref.get_doc_tokens(t.id, &t.data.content, &t.data.aliases);
                score_tetra(&t, &query_embedding, query_tokens_ref, avg_dl, doc_count, df_map_ref, false, &access_counts_snapshot, gibberish_query, &doc_tokens, now_ts_search)
            })
            .collect()
    } else {
        let label_idx = ctx.label_index.lock();
        let mut candidate_ids: HashSet<u64> = HashSet::new();
        for tok in query_tokens_ref {
            if let Some(ids) = label_idx.get(tok) {
                for &id in ids { candidate_ids.insert(id); }
            }
        }
        // 预计算 label 小写形式避免重复 to_lowercase
        for (label, ids) in label_idx.iter() {
            let label_lower = label.to_lowercase();
            if query_tokens_ref.iter().any(|w| label_lower.contains(w.as_str())) {
                for &id in ids { candidate_ids.insert(id); }
            }
        }
        drop(label_idx);

        if candidate_ids.is_empty() {
            let cap = 2000;
            candidate_ids = all_tetras.iter().take(cap).map(|t| t.id).collect();
            tracing::warn!("[Search] no label candidates, using first {} of {} tetras as fallback", cap, all_tetras.len());
        }
        all_tetras.into_iter()
            .filter(|t| candidate_ids.contains(&t.id))
            .filter(|t| passes_filters(t, filters))
            .map(|t| {
                let doc_tokens = state_ref.get_doc_tokens(t.id, &t.data.content, &t.data.aliases);
                score_tetra(&t, &query_embedding, query_tokens_ref, avg_dl, doc_count, df_map_ref, true, &access_counts_snapshot, gibberish_query, &doc_tokens, now_ts_search)
            })
            .collect()
    };

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let best_sim = scored.first().map(|s| s.1).unwrap_or(0.0);
    if gibberish_query && best_sim < 0.25 {
        tracing::info!("[Search] gibberish query '{}' best_sim={:.3} below threshold, returning empty", query, best_sim);
        return Ok(vec![]);
    }
    let need_rerank = {
        // 检索突破：放宽 LLM rerank 触发——只要 best_sim < 0.30 就触发（去掉歧义约束）
        // 之前要求 |best_sim - runner_up| < 0.05（歧义），导致"低分但决定性"的命中永远拿不到 LLM 救场
        best_sim < 0.30 && ctx.cognitive.enabled() && scored.len() > 1
    };
    if need_rerank {
        let rerank_n = scored.len().min(10);
        let cand_text: Vec<String> = scored.iter()
            .take(rerank_n)
            .enumerate()
            .map(|(i, (_id, sim, _mass, payload))| {
                let preview: String = payload.content.chars().take(120).collect();
                let labels = payload.labels.join(",");
                let alias_str = if payload.aliases.is_empty() {
                    String::new()
                } else {
                    let a: Vec<String> = payload.aliases.iter().take(3).cloned().collect();
                    format!(" [{}]", a.join("; "))
                };
                format!("[{}] [{}] sim={:.3}{} {}", i, labels, sim, alias_str, preview)
            })
            .collect();
        let cand_joined = cand_text.join("\n");
        match ctx.cognitive.rerank(query, &cand_joined) {
            Ok(ranked_indices) => {
                let mut reranked: Vec<(TetraId, f64, f64, MemoryPayload)> = Vec::new();
                for idx in ranked_indices {
                    if (idx as usize) < rerank_n {
                        reranked.push(scored[idx as usize].clone());
                    }
                }
                for item in scored.into_iter().skip(rerank_n) {
                    reranked.push(item);
                }
                scored = reranked;
            }
            Err(e) => {
                tracing::debug!("[Gateway] LLM rerank failed: {}", e);
            }
        }
    }

    scored.truncate(k);

    ctx.state.search_total.fetch_add(1, AtomicOrdering::Relaxed);
    if let Some((id, sim, _, payload)) = scored.first() {
        if *sim > 0.25 {
            ctx.state.search_hits.fetch_add(1, AtomicOrdering::Relaxed);
            // search_top_labels: 上限 200 个 label，超过时清理低频项
            {
                let mut top_labels = ctx.state.search_top_labels.lock();
                for label in &payload.labels {
                    *top_labels.entry(label.clone()).or_insert(0) += 1;
                }
                if top_labels.len() > 200 {
                    // 保留 top 150 by count
                    let mut sorted: Vec<_> = top_labels.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    sorted.sort_by(|a, b| b.1.cmp(&a.1));
                    sorted.truncate(150);
                    top_labels.clear();
                    for (k, v) in sorted { top_labels.insert(k, v); }
                }
            }
            *ctx.state.access_counts.lock().entry(*id).or_insert(0) += 1;
        }
    }
    if scored.is_empty() || scored.first().map(|s| s.1).unwrap_or(0.0) < 0.15 {
        let mut miss = ctx.state.search_miss_queries.lock();
        miss.push_back(query.to_string());
        while miss.len() > 50 {
            miss.pop_front();
        }
    }

    Ok(scored)
}

fn passes_filters(t: &crate::domain::tetra::Tetrahedron, filters: Option<&SearchFilters>) -> bool {
    let Some(f) = filters else { return true };
    if let Some(ref labels) = f.labels {
        if !labels.is_empty() {
            let has_any = labels.iter().any(|l| t.data.labels.contains(l));
            if !has_any { return false; }
        }
    }
    if let Some(min) = f.min_importance {
        if t.data.importance < min { return false; }
    }
    if let Some(max) = f.max_importance {
        if t.data.importance > max { return false; }
    }
    if let Some(since) = f.since_ts {
        if t.data.timestamp < since { return false; }
    }
    if let Some(until) = f.until_ts {
        if t.data.timestamp > until { return false; }
    }
    // D4: 时点有效性 — 存在锚点 = valid_from>0 ? valid_from : timestamp(旧数据valid_from恒0, timestamp即故事时间)
    if let Some(as_of) = f.as_of {
        let from_anchor = if t.data.valid_from > 0 { t.data.valid_from } else { t.data.timestamp };
        if from_anchor > as_of { return false; }
        if let Some(vt) = t.data.valid_to {
            if vt <= as_of { return false; }
        }
    }
    if let Some(ref project) = f.project {
        let has_project = t.data.labels.iter().any(|l| l == project || l.starts_with(&format!("project:{}", project)));
        if !has_project { return false; }
    }
    true
}

/// P1 搜索硬化: MemoryPayload 版本的 passes_filters (D1-D3 修复)
pub fn passes_filters_pub(payload: &crate::domain::tetra::MemoryPayload, f: &SearchFilters) -> bool {
    if let Some(ref labels) = f.labels {
        if !labels.is_empty() {
            let has_any = labels.iter().any(|l| payload.labels.contains(l));
            if !has_any { return false; }
        }
    }
    if let Some(min) = f.min_importance {
        if payload.importance < min { return false; }
    }
    if let Some(ref project) = f.project {
        let has_project = payload.labels.iter().any(|l| l == project || l.starts_with(&format!("project:{}", project)));
        if !has_project { return false; }
    }
    if let Some(since) = f.since_ts {
        if payload.timestamp < since { return false; }
    }
    true
}

fn compute_query_embedding(text: &str, vector: Option<&crate::engine::vector::VectorLayer>, embedding: &EmbeddingService) -> Option<Vec<f64>> {
    if let Some(vl) = vector {
        match vl.embed(text) {
            Ok(emb) => return Some(emb),
            Err(e) => tracing::warn!("[Gateway] ONNX query embed failed: {}", e),
        }
    }
    if embedding.enabled() {
        match embedding.embed(text) {
            Ok(emb) => return Some(emb),
            Err(e) => tracing::warn!("[Gateway] HTTP query embed failed: {}", e),
        }
    }
    None
}

fn hash_string(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

pub fn hash_content(s: &str) -> u64 {
    hash_string(s)
}

fn keyword_score(query_tokens: &[String], payload: &MemoryPayload) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }
    let content_lower = payload.content.to_lowercase();
    let alias_text = payload.aliases.join(" ").to_lowercase();
    let searchable = format!("{} {}", content_lower, alias_text);
    let matched = query_tokens.iter()
        .filter(|w| searchable.contains(w.as_str()))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn compute_label_boost(query_tokens: &[String], labels: &[String]) -> f64 {
    if query_tokens.is_empty() || labels.is_empty() {
        return 0.0;
    }
    let label_text = labels.join(" ").to_lowercase();
    let label_match = query_tokens.iter()
        .filter(|w| label_text.contains(w.as_str()))
        .count();
    label_match as f64 / query_tokens.len() as f64
}

fn compute_entity_boost(query_tokens: &[String], labels: &[String]) -> f64 {
    if query_tokens.is_empty() { return 0.0; }
    let entity_labels: Vec<&String> = labels.iter().filter(|l| l.starts_with("entity:")).collect();
    if entity_labels.is_empty() { return 0.0; }
    let entity_text = entity_labels.iter()
        .map(|l| l.trim_start_matches("entity:").to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let matched = query_tokens.iter()
        .filter(|w| entity_text.contains(w.as_str()))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn compute_alias_boost(query_tokens: &[String], aliases: &[String]) -> f64 {
    if aliases.is_empty() { return 0.0; }
    if query_tokens.is_empty() { return 0.0; }
    let alias_text = aliases.join(" ").to_lowercase();
    let matched = query_tokens.iter()
        .filter(|w| alias_text.contains(w.as_str()))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn bm25_score(query_tokens: &[String], doc_tokens: &[String], dl: f64, avg_dl: f64, doc_count: usize, df_map: &std::collections::HashMap<String, usize>) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }

    let k1 = 1.2;
    let b = 0.75;
    let n = doc_count.max(1) as f64;

    // 预计算 query term frequency map，避免 O(n*m) 嵌套扫描
    let mut tf_map: HashMap<&String, f64> = HashMap::new();
    for token in doc_tokens {
        *tf_map.entry(token).or_insert(0.0) += 1.0;
    }

    let mut score = 0.0;
    for term in query_tokens {
        let tf = *tf_map.get(term).unwrap_or(&0.0);
        if tf == 0.0 { continue; }
        let df = *df_map.get(term).unwrap_or(&1) as f64;
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
        let tf_norm = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_dl));
        score += idf * tf_norm;
    }
    score
}

fn is_auto_generated(labels: &[String]) -> bool {
    labels.iter().any(|l| {
        l == "reflect" || l == "session-summary" || l == "auto-extracted"
        || l == "ctx-finding" || l == "cognitive-output" || l == "auto-generated"
        || l == "dream-insight"
    })
}

fn is_noise_label(labels: &[String]) -> bool {
    labels.iter().any(|l| {
        let lower = l.to_lowercase();
        lower == "test" || lower == "testing" || lower == "junk" || lower == "scratch" || lower == "tmp" || lower == "temp"
    })
}

fn is_noise_content(content: &str) -> bool {
    let lower = content.to_lowercase();
    let trimmed = content.trim();
    if trimmed.len() < 10 {
        return true;
    }
    let noise_phrases = ["test", "testing 123", "hello world", "测试中文", "测试内容", "test content"];
    for phrase in &noise_phrases {
        if lower == *phrase || trimmed.eq_ignore_ascii_case(phrase) {
            return true;
        }
    }
    if lower.starts_with("[session]") && lower.contains("| accomplished: test") {
        return true;
    }
    if lower.starts_with("[finding]") && lower.contains("| test") {
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn score_tetra(
    t: &crate::domain::tetra::Tetrahedron,
    query_embedding: &Option<Vec<f64>>,
    query_tokens: &[String],
    avg_dl: f64,
    doc_count: usize,
    df_map: &HashMap<String, usize>,
    keyword_fallback: bool,
    access_counts: &HashMap<TetraId, u32>,
    gibberish_query: bool,
    doc_tokens: &[String],
    now_ts: f64,
) -> (TetraId, f64, f64, MemoryPayload) {
    let vec_sim = if let Some(ref qe) = query_embedding {
        if !t.data.embedding.is_empty() && t.data.embedding.len() == qe.len() {
            VectorLayer::cosine_similarity(qe, &t.data.embedding)
        } else if keyword_fallback {
            keyword_score(query_tokens, &t.data)
        } else {
            0.0
        }
    } else if keyword_fallback {
        keyword_score(query_tokens, &t.data)
    } else {
        0.0
    };
    let dl = doc_tokens.len() as f64;
    let bm25 = bm25_score(query_tokens, doc_tokens, dl, avg_dl, doc_count, df_map);
    let has_exact_match = has_exact_keyword_match(query_tokens, &t.data);
    let bm25_norm = (bm25 / (bm25 + 1.0)) * if has_exact_match { 0.40 } else { 0.30 };  // 检索突破：无精确匹配从0.20→0.35，让BM25能补偿向量失败
    let hybrid = vec_sim * 0.55 + bm25_norm;
    let label_boost = compute_label_boost(query_tokens, &t.data.labels);
    let entity_boost = compute_entity_boost(query_tokens, &t.data.labels);
    let alias_boost = compute_alias_boost(query_tokens, &t.data.aliases);
    // 检索突破：从乘性 penalty 链改为加性信号融合
    // 乘性链问题：0.3 * 0.84 * 0.5 * 0.7 = 0.088——弱向量匹配被 importance/recency/auto_penalty 压到接近 0
    // 加性模型：各信号独立贡献，不会互相压制
    let age_days = ((now_ts - t.data.timestamp as f64) / 86400.0).max(0.0);
    let recency = (-age_days * 0.001).exp();
    let importance_norm = (t.data.importance / 3.5).min(1.0); // 归一化到 0-1
    let access_count = *access_counts.get(&t.id).unwrap_or(&0) as f64;
    let access_bonus = (access_count / (access_count + 5.0)) * 0.1;

    // 检索突破2: 精确子串匹配加分 — 当查询文本直接出现在记忆内容中时给予强加分
    // 这是自引用检索的最强信号：搜索记忆自身内容时必定触发
    let exact_substring_boost = {
        if query_tokens.len() >= 3 {
            // 重建查询原文（取前8个token拼接，用于子串检测）
            let query_fragment: String = query_tokens.iter().take(8)
                .cloned().collect::<Vec<_>>().join(" ");
            let content_lower = t.data.content.to_lowercase();
            // 检查查询片段是否作为子串出现在内容中
            if query_fragment.len() > 10 && content_lower.contains(&query_fragment) {
                0.25  // 检索突破3: 强加分提升到0.25——足以压过向量噪声
            } else {
                // 也检查token级别的重合度（无序匹配）
                let content_tokens = doc_tokens;
                let query_set: std::collections::HashSet<&str> = query_tokens.iter().take(10).map(|s| s.as_str()).collect();
                let doc_set: std::collections::HashSet<&str> = content_tokens.iter().map(|s| s.as_str()).collect();
                let overlap = query_set.intersection(&doc_set).count();
                let overlap_ratio = overlap as f64 / query_set.len().max(1) as f64;
                if overlap_ratio > 0.5 {  // 50%+ token重合（降低阈值捕获更多匹配）
                    0.12 * overlap_ratio  // 检索突破3: token重叠信号增强
                } else {
                    0.0
                }
            }
        } else {
            0.0
        }
    };

    // 加性融合：向量+BM25 混合分作为主体（55%），信号作为加分项
    // 检索突破3: exact_substring匹配时hybrid权重×1.2（让精确匹配的记忆更靠前）
    let hybrid_weight = if exact_substring_boost > 0.15 { 0.60 } else { 0.50 };
    let mut score = hybrid * hybrid_weight;
    score += label_boost * 0.10;
    score += entity_boost * 0.10;
    score += alias_boost * 0.08;
    score += importance_norm * 0.08;
    score += recency * 0.04;
    score += access_bonus;
    score += exact_substring_boost;

    // 惩罚项改为减法（而非乘法），避免归零
    let penalty = if t.data.labels.iter().any(|l| l.starts_with("meta-")) { 0.1 } else { 0.0 };
    let noise_penalty = if is_noise_label(&t.data.labels) || is_noise_content(&t.data.content) { 0.2 } else { 0.0 };
    let auto_penalty = if is_auto_generated(&t.data.labels) { 0.05 } else { 0.0 };
    let validity_penalty = if t.data.valid_to.is_some() { 0.3 } else { 0.0 };
    score -= (penalty + noise_penalty + auto_penalty + validity_penalty);
    score = score.max(0.0).min(1.0);

    (t.id, score, t.mass, t.data.clone())
}

fn has_exact_keyword_match(query_tokens: &[String], payload: &MemoryPayload) -> bool {
    if query_tokens.is_empty() { return false; }
    let content_lower = payload.content.to_lowercase();
    let all_text = format!("{} {} {}", content_lower, payload.aliases.join(" ").to_lowercase(), payload.labels.join(" ").to_lowercase());
    let match_count = query_tokens.iter()
        .filter(|t| all_text.contains(t.as_str()))
        .count();
    match_count as f64 / query_tokens.len() as f64 > 0.5
}

/// Phase 1+: exact 模式专用 — 只要任一 token 命中即返回 true
///
/// 与 has_exact_keyword_match 的区别:后者要求 >50% token 命中(适合 hybrid 区分相关度),
/// exact 模式契约是"包含特定标识符的记忆都要返回",所以任一命中即可。
/// BM25 分数自然决定排序(含更多 token 的分数更高)。
fn has_any_token_match(query_tokens: &[String], payload: &MemoryPayload) -> bool {
    if query_tokens.is_empty() { return false; }
    let content_lower = payload.content.to_lowercase();
    let all_text = format!("{} {} {}", content_lower, payload.aliases.join(" ").to_lowercase(), payload.labels.join(" ").to_lowercase());
    query_tokens.iter().any(|t| all_text.contains(t.as_str()))
}

/// Phase 1: 计算一条结果的命中来源(可解释性)
/// 返回哪些信号匹配了查询 token, 用于 exact 模式的 matched_by 字段
fn compute_matched_by(query_tokens: &[String], payload: &MemoryPayload, doc_tokens: &[String]) -> MatchedBy {
    let mut sources: MatchedBy = Vec::new();
    if query_tokens.is_empty() { return sources; }

    // BM25 命中: query token 出现在文档 token 集合里
    let doc_set: HashSet<&str> = doc_tokens.iter().map(|s| s.as_str()).collect();
    let bm25_hits = query_tokens.iter().filter(|t| doc_set.contains(t.as_str())).count();
    if bm25_hits > 0 {
        sources.push("bm25");
    }

    // alias 命中
    if !payload.aliases.is_empty() {
        let alias_text = payload.aliases.join(" ").to_lowercase();
        let alias_hits = query_tokens.iter().filter(|t| alias_text.contains(t.as_str())).count();
        if alias_hits > 0 {
            sources.push("alias");
        }
    }

    // label 命中
    if !payload.labels.is_empty() {
        let label_text = payload.labels.join(" ").to_lowercase();
        let label_hits = query_tokens.iter().filter(|t| label_text.contains(t.as_str())).count();
        if label_hits > 0 {
            sources.push("label");
        }
    }

    // 精确子串命中(最强信号): query 原文片段出现在 content 里
    if query_tokens.len() >= 2 {
        let query_fragment: String = query_tokens.iter().take(6)
            .cloned().collect::<Vec<_>>().join(" ");
        let content_lower = payload.content.to_lowercase();
        if query_fragment.len() > 8 && content_lower.contains(&query_fragment) {
            sources.push("exact_substring");
        }
    }

    sources
}

/// Phase 1 检索可信度重建: Exact 模式专用评分函数
///
/// 设计契约:
/// - 纯 BM25 评分, 完全旁路向量通道(不读 embedding, 不算 cosine)
/// - BM25×10 后用饱和函数归一化到 0-1(bm25/(bm25+5.0), 软拐点=5)
/// - 无精确 keyword 命中直接归零(要么精确命中要么不返回)
/// - alias 命中给予强加权(置顶效果)
/// - 返回 5-tuple, 末尾带 MatchedBy 命中来源
#[allow(clippy::too_many_arguments)]
fn score_tetra_exact(
    t: &crate::domain::tetra::Tetrahedron,
    query_tokens: &[String],
    avg_dl: f64,
    doc_count: usize,
    df_map: &HashMap<String, usize>,
    doc_tokens: &[String],
) -> (TetraId, f64, f64, MemoryPayload, MatchedBy) {
    let dl = doc_tokens.len() as f64;
    let bm25 = bm25_score(query_tokens, doc_tokens, dl, avg_dl, doc_count, df_map);
    let has_exact = has_any_token_match(query_tokens, &t.data);

    // 无精确命中直接归零 — exact 模式的核心契约(任一 token 命中即可)
    if !has_exact {
        return (t.id, 0.0, t.mass, t.data.clone(), Vec::new());
    }

    // BM25×10 饱和归一化: bm25 原始分约 0-10, ×10 后 0-100, 用 /(x+5.0) 归一化
    // bm25=5 → 0.5, bm25=20 → 0.8, bm25=50 → 0.91
    let bm25_boosted = bm25 * 10.0;
    let mut score = bm25_boosted / (bm25_boosted + 5.0);

    // alias 命中置顶: alias 命中额外 +0.3(足以让 alias 命中的排到纯 content 命中前面)
    let alias_boost = compute_alias_boost(query_tokens, &t.data.aliases);
    if alias_boost > 0.0 {
        score += alias_boost * 0.3;
    }

    // label 命中小幅加权
    let label_boost = compute_label_boost(query_tokens, &t.data.labels);
    if label_boost > 0.0 {
        score += label_boost * 0.05;
    }

    // 惩罚项(与 hybrid 一致: 噪声/过期记忆降权)
    let noise_penalty = if is_noise_label(&t.data.labels) || is_noise_content(&t.data.content) { 0.3 } else { 0.0 };
    let validity_penalty = if t.data.valid_to.is_some() { 0.3 } else { 0.0 };
    score -= (noise_penalty + validity_penalty);
    score = score.max(0.0).min(1.0);

    let matched_by = compute_matched_by(query_tokens, &t.data, doc_tokens);
    (t.id, score, t.mass, t.data.clone(), matched_by)
}
