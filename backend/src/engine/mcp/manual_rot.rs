use super::McpHandler;
use std::collections::{BTreeMap, BTreeSet};

const MANUAL: &str = "backend/system_skills/epicode_system_manual.md";

fn repo_file(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every executor-facing manual: system skills + docs an agent is sent to.
fn manual_files() -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("system_skills"))
            .unwrap()
            .filter_map(|e| {
                let n = e.ok()?.file_name().into_string().ok()?;
                n.ends_with(".md")
                    .then(|| format!("backend/system_skills/{n}"))
            })
            .collect();
    v.sort();
    v.extend(
        [
            "frontend/public/llms.txt",
            "frontend/public/docs.md",
            "docs/mcp-protocol.md",
            "docs/api-reference.md",
            "docs/architecture.md",
            "docs/deployment.md",
            "README.md",
            "README.zh.md",
        ]
        .map(String::from),
    );
    v
}

struct Catalog {
    tools: BTreeMap<String, BTreeSet<String>>,
    all_params: BTreeSet<String>,
}

fn catalog() -> Catalog {
    let mut tools = BTreeMap::new();
    let mut all_params = BTreeSet::new();
    for t in McpHandler::tool_catalog()["tools"].as_array().unwrap() {
        let props: BTreeSet<String> = t["inputSchema"]["properties"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        all_params.extend(props.iter().cloned());
        tools.insert(t["name"].as_str().unwrap().to_string(), props);
    }
    Catalog { tools, all_params }
}

/// Prefixes of the MCP tool families: an identifier with one of these that
/// is neither a tool nor a parameter is a stale/invented tool reference.
const TOOL_PREFIXES: &[&str] = &[
    "memory_",
    "task_",
    "action_",
    "worker_",
    "drive_",
    "skill_",
    "skills_",
    "identity_",
    "ctx_",
    "pattern_",
    "doc_",
    "session_",
    "kg_",
    "blocks_",
    "library_",
    "dream_",
    "space_",
    "knowledge_",
    "context_",
    "enforced_",
    "embedding_",
    "epicode_",
    "decision_",
    "feedback_",
    "project_",
    "bug_",
];
/// Response fields / enum values / permission names that share a tool prefix.
const NON_TOOL_IDENTIFIERS: &[&str] = &[
    "task_pulse",
    "task_completed",
    "task_partial",
    "task_failed",
    "task_id",
    "memory_read",
    "memory_write",
    "memory_class",
    "memory_count",
    "memory_ids",
    "skill_manage",
    "skill_hint",
    "knowledge_card_used",
    "action_card_schema",
    "action_tool_surface",
    "space_stats",
    "drive_id",
    "session_id",
    "skills_version",
    "skills_updated",
    "project_list",
    "task_status",
    "epicode_session",
    "drive_queue",
    "drive_sse_signal",
    "identity_info",
];

fn is_ident(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
}

/// (identifier, args-or-None) for every `ident(` call and `` `ident` ``.
fn references(text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        let prev_ok = i == 0 || !is_ident(chars[i - 1].1);
        if c.is_ascii_lowercase() && prev_ok {
            let mut j = i;
            while j < chars.len() && is_ident(chars[j].1) {
                j += 1;
            }
            let end = chars.get(j).map(|x| x.0).unwrap_or(text.len());
            let name = &text[start..end];
            let backticked =
                i > 0 && chars[i - 1].1 == '`' && chars.get(j).map(|x| x.1) == Some('`');
            if chars.get(j).map(|x| x.1) == Some('(') {
                // balanced args on one line
                let mut depth = 0i32;
                let mut k = j;
                let mut args = None;
                while k < chars.len() && chars[k].1 != '\n' {
                    match chars[k].1 {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' => {
                            depth -= 1;
                            if depth == 0 {
                                args = Some(text[chars[j].0 + 1..chars[k].0].to_string());
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                out.push((name.to_string(), args));
            } else if backticked {
                out.push((name.to_string(), None));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Top-level argument names of `a, b=1, c: [..]` (depth 0 only).
/// `strict` also treats bare identifiers as parameter names.
fn arg_names(args: &str, strict: bool) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut quoted = false;
    for c in args.chars() {
        match c {
            '"' | '\'' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted => depth -= 1,
            ',' if depth == 0 && !quoted => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
        .iter()
        .filter_map(|p| {
            let p = p.trim();
            let name: String = p.chars().take_while(|&c| is_ident(c)).collect();
            if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_lowercase()) {
                return None;
            }
            let rest = p[name.len()..].trim_start();
            if rest.starts_with('=') || rest.starts_with(':') || (strict && rest.is_empty()) {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

/// Route table from cloud.rs: normalized path -> methods.
fn routes() -> BTreeMap<String, BTreeSet<String>> {
    let src = repo_file("backend/src/bin/cloud.rs");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for chunk in src.split(".route(").skip(1) {
        let Some(rest) = chunk.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let body = &rest[end..];
        let body = &body[..body.find(".layer(").unwrap_or(body.len())];
        let e = out.entry(norm(&rest[..end])).or_default();
        for m in ["get", "post", "put", "delete", "patch"] {
            if body.match_indices(&format!("{m}(")).any(|(i, _)| {
                i == 0 || {
                    let b = body.as_bytes()[i - 1];
                    !b.is_ascii_alphanumeric() && b != b'_'
                }
            }) {
                e.insert(m.to_uppercase());
            }
        }
    }
    out
}

fn norm(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with(':') || (seg.starts_with('{') && seg.ends_with('}')) {
                ":p"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `METHOD /path` mentions (public `/api` prefix and host stripped).
fn endpoint_refs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for m in ["GET", "POST", "PUT", "DELETE", "PATCH"] {
        for (i, _) in text.match_indices(&format!("{m} ")) {
            if i > 0 && text.as_bytes()[i - 1].is_ascii_alphanumeric() {
                continue;
            }
            let rest = &text[i + m.len() + 1..];
            let rest = rest.strip_prefix("https://epicode.cn").unwrap_or(rest);
            if !rest.starts_with('/') {
                continue;
            }
            let path: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "/:_-{}.".contains(*c))
                .collect();
            let path = path.trim_end_matches('.').to_string();
            let path = path.strip_prefix("/api").map(String::from).unwrap_or(path);
            if path.is_empty() || path == "/" {
                continue;
            }
            out.push((m.to_string(), path));
        }
    }
    out
}

fn check_file(
    rel: &str,
    text: &str,
    cat: &Catalog,
    routes: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let strict = rel == MANUAL;
    let mut bad = Vec::new();
    for (name, args) in references(text) {
        if let Some(props) = cat.tools.get(&name) {
            if let Some(a) = args {
                for p in arg_names(&a, strict) {
                    if !props.contains(&p) {
                        bad.push(format!("{rel}: {name}({p}) — no such parameter"));
                    }
                }
            }
        } else if name.contains('_')
            && TOOL_PREFIXES.iter().any(|p| name.starts_with(p))
            && !cat.all_params.contains(&name)
            && !NON_TOOL_IDENTIFIERS.contains(&name.as_str())
        {
            bad.push(format!("{rel}: `{name}` — no such tool"));
        }
    }
    for (m, path) in endpoint_refs(text) {
        let ok = routes.get(&norm(&path)).is_some_and(|ms| ms.contains(&m));
        if !ok {
            bad.push(format!("{rel}: {m} {path} — no such route/method"));
        }
    }
    bad
}

#[test]
fn manuals_reference_only_live_tools_params_and_endpoints() {
    let cat = catalog();
    let routes = routes();
    assert!(
        cat.tools.len() >= 60 && routes.len() >= 50,
        "extraction broke"
    );
    let mut bad = Vec::new();
    let mut scanned = 0;
    for rel in manual_files() {
        let text = repo_file(&rel);
        if text.contains("Historical document") {
            continue;
        }
        scanned += 1;
        bad.extend(check_file(&rel, &text, &cat, &routes));
    }
    assert!(scanned >= 15, "too few manuals scanned ({scanned})");
    assert!(
        bad.is_empty(),
        "stale manual references:\n{}",
        bad.join("\n")
    );
}

#[test]
fn checker_catches_stale_references() {
    let cat = catalog();
    let routes = routes();
    let bad = check_file(
        MANUAL,
        "call `knowledge_graph` then identity_step(agent_name) and GET /api/v1/stats/public; \
             memory_recall(query, limit) and GET /api/v1/memories/:id are fine",
        &cat,
        &routes,
    );
    assert_eq!(bad.len(), 3, "{bad:#?}");
}

#[test]
fn canonical_manual_covers_catalog_and_version() {
    let src = repo_file(MANUAL);
    assert!(
        src.lines().next().unwrap().contains("v{{EPICODE_VERSION}}"),
        "manual title must carry the build-version placeholder"
    );
    // No hard-coded semver: versions come from the build.
    let words: Vec<&str> = src
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .collect();
    let semver: Vec<&&str> = words
        .iter()
        .filter(|w| w.split('.').filter(|p| !p.is_empty()).count() == 3)
        .collect();
    assert!(
        semver.is_empty(),
        "hard-coded versions in manual: {semver:?}"
    );
    let missing: Vec<String> = catalog()
        .tools
        .keys()
        .filter(|t| !src.contains(&format!("`{t}`")))
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "manual checklist misses tools: {missing:?}"
    );
    let served = McpHandler::system_manual();
    assert!(!served.contains("{{"), "unsubstituted placeholder");
    assert_eq!(
        McpHandler::manual_version(&served),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn handshake_and_skill_get_serve_the_manual() {
    let h = McpHandler::new(std::sync::Arc::new(super::tests::isolated_engine()));
    let call = |name: &str, args: serde_json::Value| {
        h.handle(super::McpRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({"name": name, "arguments": args})),
        })
        .result
        .unwrap()["structuredContent"]
            .clone()
    };
    call(
        "identity_confirm",
        serde_json::json!({"name": "rot", "mission": "test", "author": "ci"}),
    );
    let hs = call("epicode_handshake", serde_json::json!({"agent_id": "rot"}));
    let sk = &hs["data"]["system_skill"];
    assert_eq!(sk["version"], env!("CARGO_PKG_VERSION"), "{hs}");
    let content = sk["content"].as_str().unwrap();
    assert!(content.starts_with("# Epicode 执行者手册"), "{content:.80}");
    assert!(
        content == McpHandler::system_manual(),
        "handshake manual differs"
    );
    let got = call("skill_get", serde_json::json!({"name": "epicode-system"}));
    assert!(
        got["data"]["skill_md"].as_str() == Some(content),
        "skill_get(epicode-system) must serve the same manual"
    );
}
