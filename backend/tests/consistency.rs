//! 一致性契约测试(2026-09-28迭代): 守住三个一致性面
//! 1. 前后端一致: 前端api.ts调用的每个端点必须存在于后端路由注册表
//! 2. MCP服务一致: tools_list声明的每个工具必须有dispatch分支(反之亦然)
//! 3. SMRP信封一致: MCP与REST共用信封构造, 关键字段不漂移
//!
//! 这些测试读源码做静态契约(而非运行时) — 破坏一致性时CI先红, 不等线上404。

use std::collections::BTreeSet;
use std::path::Path;

/// 字符串常量(避开测试源码里的引号转义地狱)
const COLON_PARAM: &str = "/:";
const PARAM_ID: &str = "/:id";

fn read(rel: &str) -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    std::fs::read_to_string(base.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// 从 cloud.rs 提取全部已注册路由(含跨行格式) — 字节扫描, 无引号字面量
fn backend_routes() -> BTreeSet<String> {
    let src = read("backend/src/bin/cloud.rs");
    let b = src.as_bytes();
    let needle = b".route(";
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i + needle.len() <= b.len() {
        if &b[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            while j < b.len() && (b[j] == 0x20 || b[j] == 0x0a || b[j] == 0x0d || b[j] == 0x09) {
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                let start = j + 1;
                let mut k = start;
                while k < b.len() && b[k] != b'"' {
                    k += 1;
                }
                if k < b.len() {
                    out.insert(src[start..k].to_string());
                }
            }
            i = j;
        }
        i += 1;
    }
    out
}

fn extract_frontend_endpoints(api: &str) -> BTreeSet<String> {
    // 无字符字面量写法: 按 request 定位, 在其后120字符窗口里找 '/...' 单引号串
    let mut called = BTreeSet::new();
    let b = api.as_bytes();
    let needle = b"request";
    let mut i = 0;
    while i + 7 <= b.len() {
        if &b[i..i + 7] == needle {
            let win_end = (i + 120).min(b.len());
            let mut j = i + 7;
            while j < win_end {
                if b[j] == 0x27 {
                    // 找配对单引号
                    if let Some(rel_end) = api.as_bytes()[j + 1..win_end]
                        .iter()
                        .position(|&c| c == 0x27)
                    {
                        let inner = &api[j + 1..j + 1 + rel_end];
                        if inner.starts_with('/') && !inner.contains(' ') {
                            called.insert(inner.to_string());
                        }
                    }
                    break;
                }
                j += 1;
            }
            i += 7;
        }
        i += 1;
    }
    called
}

#[test]
fn frontend_api_calls_have_backend_routes() {
    let api = read("frontend/src/lib/api.ts");
    let called = extract_frontend_endpoints(&api);
    assert!(!called.is_empty(), "前端端点提取失败(检查api.ts格式)");

    let routes = backend_routes();
    let mut missing = Vec::new();
    for c in &called {
        let hit = routes.iter().any(|r| {
            r == c
                || r.split(COLON_PARAM).next() == Some(c.as_str())
                || c.starts_with(&format!("{}/", r.trim_end_matches(PARAM_ID)))
        });
        if !hit {
            missing.push(c.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "前端调用但后端无路由(前后端断链): {missing:?}
后端路由数: {}",
        routes.len()
    );
}

#[test]
fn mcp_tools_list_matches_dispatch() {
    let src = read("backend/src/engine/mcp.rs");
    // dispatch分支: 行首 "name" => self.tool_xxx( — 仅工具方法(排除RPC方法)
    let dispatch: BTreeSet<String> = src
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if !t.contains("=> self.tool_") {
                return None;
            }
            t.strip_prefix('"')
                .and_then(|r| r.split('"').next())
                .filter(|n| !n.is_empty())
                .map(String::from)
        })
        .collect();
    // tools_list声明体: fn tools_list 到下一个fn(非60K窗口)
    let tl_start = src.find("fn tools_list").expect("tools_list not found");
    let tl_end = src[tl_start..]
        .find(
            "
    fn ",
        )
        .map(|e| tl_start + e)
        .unwrap_or(src.len());
    let tl_body = &src[tl_start..tl_end];
    let mut declared = BTreeSet::new();
    let mut rest = tl_body;
    while let Some(pos) = rest.find("\"name\": \"") {
        let after = &rest[pos + 9..];
        if let Some(close) = after.find('"') {
            let name = &after[..close];
            if !name.is_empty() {
                declared.insert(name.to_string());
            }
        }
        rest = &after[1.min(after.len())..];
    }
    assert!(
        !dispatch.is_empty() && !declared.is_empty(),
        "dispatch={} declared={}",
        dispatch.len(),
        declared.len()
    );
    let undeclared: Vec<_> = dispatch.difference(&declared).collect();
    let unimplemented: Vec<_> = declared.difference(&dispatch).collect();
    assert!(
        undeclared.is_empty() && unimplemented.is_empty(),
        "MCP工具清单不一致 — dispatch有未声明: {undeclared:?}, 声明无dispatch: {unimplemented:?}"
    );
}

#[test]
fn smrp_envelope_shared_fields_present() {
    let smrp = read("backend/src/engine/smrp.rs");
    // 信封关键字段: 成功信封必须携带的结构
    for field in ["schema_version", "tool", "ok", "data", "status"] {
        assert!(
            smrp.contains(&format!("\"{field}\"")),
            "SMRP信封缺关键字段: {field}"
        );
    }
    // REST与MCP共用层存在(§1.3 承诺)
    assert!(smrp.contains("envelope_ok"), "SMRP共用信封构造缺失");
}

#[test]
fn cloud_mcp_transports_share_handler_and_access_gates() {
    let http = read("backend/src/bin/cloud/mcp_endpoint.rs");
    let tcp = read("backend/src/bin/cloud/tcp.rs");
    for (transport, source) in [("HTTP", http.as_str()), ("TCP", tcp.as_str())] {
        for contract in [
            "McpHandler::with_pub_skills",
            "QuotaContext",
            "guard_mcp_request",
            "start_cognitive_loop_if_needed",
            "process_json",
        ] {
            assert!(
                source.contains(contract),
                "{transport} MCP transport is missing shared contract {contract}"
            );
        }
    }
    assert!(
        tcp.contains("read_mcp_line"),
        "TCP must use bounded MCP line reads"
    );
    assert!(
        http.contains("MAX_MCP_REQUEST_BYTES"),
        "HTTP must use the shared MCP request-size limit"
    );
    assert!(
        http.contains("is_notification") && http.contains("StatusCode::ACCEPTED"),
        "HTTP notifications must be acknowledged without a JSON-RPC response body"
    );
}

/// Permission names: the console's grant list must use exactly the spellings
/// `Permission::parse` accepts (a typo turns into a 400 "unknown permission").
#[test]
fn frontend_permission_names_match_backend() {
    use epicode::engine::user_manager::Permission;
    let backend: BTreeSet<String> = Permission::all()
        .into_iter()
        .map(|p| p.as_str().to_string())
        .collect();
    let api = read("frontend/src/lib/api.ts");
    let start = api
        .find("export const PERMISSIONS")
        .expect("PERMISSIONS list in api.ts");
    let body = &api[start..start + api[start..].find("];").expect("PERMISSIONS end")];
    let frontend: BTreeSet<String> = body
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(String::from)
        .collect();
    assert_eq!(
        frontend, backend,
        "frontend PERMISSIONS drifted from Permission::as_str"
    );

    let page = read("frontend/src/pages/DashboardPermissions.tsx");
    for name in &backend {
        assert!(
            page.contains(&format!("{name}: {{")),
            "DashboardPermissions.tsx has no label for {name}"
        );
    }
}

/// Route table with HTTP methods: path -> {GET, POST, ...} (static scan of cloud.rs).
fn backend_route_methods() -> std::collections::BTreeMap<String, BTreeSet<String>> {
    let src = read("backend/src/bin/cloud.rs");
    let mut out: std::collections::BTreeMap<String, BTreeSet<String>> = Default::default();
    let chunks: Vec<&str> = src.split(".route(").skip(1).collect();
    for chunk in chunks {
        let t = chunk.trim_start();
        let Some(rest) = t.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let path = rest[..end].to_string();
        let body = &rest[end..];
        let body = &body[..body.find(".layer(").unwrap_or(body.len())];
        let entry = out.entry(path).or_default();
        for m in ["get", "post", "put", "delete", "patch"] {
            let needle = format!("{m}(");
            let hit = body.match_indices(&needle).any(|(i, _)| {
                i == 0
                    || !body.as_bytes()[i - 1].is_ascii_alphanumeric()
                        && body.as_bytes()[i - 1] != b'_'
            });
            if hit {
                entry.insert(m.to_uppercase());
            }
        }
    }
    out
}

/// The public API docs page (docs-endpoints.ts) must only document routes that
/// exist with the documented HTTP method.
#[test]
fn documented_endpoints_exist_with_method() {
    let routes = backend_route_methods();
    let docs = read("frontend/src/lib/docs-endpoints.ts");
    let mut checked = 0;
    let mut bad = Vec::new();
    for seg in docs.split("method: '").skip(1) {
        let method = &seg[..seg.find('\'').unwrap()];
        let Some(p) = seg.find("path: '") else {
            continue;
        };
        let path_full = &seg[p + 7..];
        let path = &path_full[..path_full.find('\'').unwrap()];
        let path = path.split('?').next().unwrap();
        checked += 1;
        match routes.get(path) {
            Some(ms) if ms.contains(method) => {}
            other => bad.push(format!("{method} {path} -> {other:?}")),
        }
    }
    assert!(checked >= 20, "docs endpoint extraction broke ({checked})");
    assert!(
        bad.is_empty(),
        "documented endpoints missing/wrong method: {bad:?}"
    );
}

/// REST and MCP "delete" must share one implementation (permanent purge,
/// `mode: "purge"`); soft forgetting lives only on the explicit forget paths.
#[test]
fn rest_and_mcp_delete_share_purge_semantics() {
    fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
        let s = src.find(sig).unwrap_or_else(|| panic!("{sig} not found"));
        let rest = &src[s + sig.len()..];
        let e = ["\npub async fn ", "\npub fn ", "\n    fn ", "\n    pub fn "]
            .iter()
            .filter_map(|n| rest.find(n))
            .min()
            .unwrap_or(rest.len());
        &rest[..e]
    }
    let rest_src = read("backend/src/bin/cloud/memory.rs");
    let mcp_src = read("backend/src/engine/mcp.rs");
    for (name, body) in [
        (
            "REST delete_memory",
            fn_body(&rest_src, "pub async fn delete_memory("),
        ),
        (
            "REST batch_delete_memories",
            fn_body(&rest_src, "pub async fn batch_delete_memories("),
        ),
        (
            "MCP memory_delete",
            fn_body(&mcp_src, "fn tool_memory_delete("),
        ),
    ] {
        assert!(
            body.contains("api_delete_memory("),
            "{name} must use scheduler.api_delete_memory"
        );
        assert!(
            !body.contains("api_forget_memory("),
            "{name} must not soft-forget"
        );
    }
    assert_eq!(epicode::engine::scheduler::DELETE_MODE, "purge");
    let forget = fn_body(&rest_src, "pub async fn forget_memory(");
    assert!(
        forget.contains("api_forget_memory("),
        "REST forget path must stay recoverable"
    );
}
